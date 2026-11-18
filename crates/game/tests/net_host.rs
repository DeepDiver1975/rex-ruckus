//! Hosting a co-op game: a host app driven by hand and raw `rr_net::Client`s over real UDP on
//! 127.0.0.1.

use std::net::SocketAddr;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::actors::Target;
use rr_core::combat::{CombatEvent, PendingBlast, Via};
use rr_core::defs::SplashDef;
use rr_core::defs::{Defs, WeaponId};
use rr_core::difficulty::Difficulty;
use rr_core::explosion::Blast;
use rr_core::map::{ActorKind, ActorSpawn, Item, ItemKind, Key, Map};
use rr_core::players::PlayerId;
use rr_core::projectile::Shooter;
use rr_core::weapons::WeaponEvent;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerVitals, insert_defs,
};
use rr_game::flow::{FlowPlugin, LevelFile, restart_level};
use rr_game::level::CurrentMap;
use rr_game::level::load_map;
use rr_game::mechanics::use_key;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::net::host::{
    HELLO_TIMEOUT, MAX_INPUT_QUEUE, NetHost, NetHostPlugin, NetIdentity, QUEUE_SLACK,
    SNAPSHOT_EVERY, apply_remote_input, start_hosting,
};
use rr_game::player::{Look, PendingInput, Player, PlayerBody, PlayerSimPlugin, PrevFeet};
use rr_net::*;

const STEP: Duration = Duration::from_millis(16);

fn host_app_with(edit: impl FnOnce(&mut Map)) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    // Only the test's own `FixedUpdate` runs tick the sim.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let mut map = load_map("test_yard.ron");
    edit(&mut map);
    insert_level(&mut app, map, Difficulty::Normal);
    app.insert_resource(LevelFile("test_yard.ron".into()));
    let mut defs = Defs::builtin();
    for e in &mut defs.enemies {
        e.aim_time = 0.0;
    }
    insert_defs(&mut app, defs);
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        NetHostPlugin,
    ));
    app.update();
    start_hosting(
        app.world_mut(),
        "127.0.0.1:0".parse().unwrap(),
        "Host".into(),
    )
    .expect("host");
    app
}

fn host_app() -> App {
    host_app_with(|_| {})
}

fn host_addr(app: &App) -> SocketAddr {
    app.world().resource::<NetHost>().host.local_addr()
}

/// A raw client and everything it received.
struct Peer {
    client: Client,
    got: Vec<ServerMsg>,
}

impl Peer {
    fn control(&self) -> Vec<&ServerMsg> {
        self.got
            .iter()
            .filter(|m| !matches!(m, ServerMsg::Snapshot { .. }))
            .collect()
    }

    fn snapshots(&self) -> Vec<(PlayerId, &rr_core::snapshot::Snapshot)> {
        self.got
            .iter()
            .filter_map(|m| match m {
                ServerMsg::Snapshot { you, snap } => Some((*you, snap)),
                _ => None,
            })
            .collect()
    }

    fn loads(&self) -> Vec<Load> {
        self.got
            .iter()
            .filter_map(|m| match m {
                ServerMsg::Load(l) => Some(l.clone()),
                _ => None,
            })
            .collect()
    }

    fn refusal(&self) -> Option<Refusal> {
        self.got.iter().find_map(|m| match m {
            ServerMsg::Refused(r) => Some(*r),
            _ => None,
        })
    }

    fn send(&mut self, msg: &ClientMsg) {
        self.client.send(msg);
        self.client.flush();
    }
}

/// One round: a frame, one fixed tick, then every client. Returns the tick's weapon events.
fn pump(app: &mut App, peers: &mut [&mut Peer]) -> Vec<(PlayerId, WeaponEvent)> {
    app.update();
    app.world_mut().run_schedule(FixedUpdate);
    let fired = std::mem::take(&mut app.world_mut().resource_mut::<FxQueue>().weapon);
    for p in peers.iter_mut() {
        let msgs = p.client.update(STEP);
        p.got.extend(msgs);
        p.client.flush();
    }
    std::thread::sleep(Duration::from_millis(1));
    fired
}

/// Pumps until `done` (at most 300 rounds); panics with `what` otherwise.
fn pump_until(
    app: &mut App,
    peers: &mut [&mut Peer],
    what: &str,
    done: impl Fn(&mut App, &[&mut Peer]) -> bool,
) {
    for _ in 0..300 {
        pump(app, peers);
        if done(app, peers) {
            return;
        }
    }
    panic!("timed out waiting for {what}");
}

fn connect(app: &mut App) -> Peer {
    let mut peer = Peer {
        client: Client::connect(host_addr(app)).expect("client"),
        got: Vec::new(),
    };
    pump_until(app, &mut [&mut peer], "a connection", |_, p| {
        p[0].client.state() == ClientState::Connected
    });
    peer
}

fn hello(app: &App, name: &str) -> Hello {
    let id = app.world().resource::<NetIdentity>();
    Hello {
        protocol: PROTOCOL_VERSION,
        game_version: id.game_version.clone(),
        assets: id.assets,
        name: name.into(),
    }
}

/// Connects and says `hello`; returns once the host answered.
fn greet(app: &mut App, hello: Hello) -> Peer {
    let mut peer = connect(app);
    peer.send(&ClientMsg::Hello(hello));
    pump_until(app, &mut [&mut peer], "an answer", |_, p| {
        p[0].got
            .iter()
            .any(|m| matches!(m, ServerMsg::Welcome(_) | ServerMsg::Refused(_)))
    });
    peer
}

/// Joins as `name` and waits for the level load.
fn join(app: &mut App, name: &str) -> Peer {
    let h = hello(app, name);
    let mut peer = greet(app, h);
    pump_until(app, &mut [&mut peer], "a load", |_, p| {
        !p[0].loads().is_empty()
    });
    peer
}

fn player_ids(app: &mut App) -> Vec<u8> {
    let mut q = app.world_mut().query::<&Player>();
    let mut ids: Vec<u8> = q.iter(app.world()).map(|p| p.id.0).collect();
    ids.sort();
    ids
}

fn entity(app: &mut App, id: PlayerId) -> Entity {
    let mut q = app.world_mut().query::<(Entity, &Player)>();
    q.iter(app.world())
        .find(|(_, p)| p.id == id)
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no player {id:?}"))
}

fn pos(app: &mut App, id: PlayerId) -> Vec3 {
    let e = entity(app, id);
    app.world().get::<PlayerBody>(e).unwrap().0.pos
}

fn angle(app: &mut App, id: PlayerId) -> f32 {
    let e = entity(app, id);
    app.world().get::<Look>(e).unwrap().angle
}

fn seats(app: &App) -> usize {
    app.world().resource::<NetHost>().seats.len()
}

fn frame(seq: u32) -> InputFrame {
    InputFrame {
        seq,
        forward: 0.0,
        strafe: 0.0,
        angle: 0.0,
        pitch: 0.0,
        buttons: 0,
        select: None,
        cycle: 0,
    }
}

fn input(peer: &mut Peer, f: InputFrame) {
    peer.send(&ClientMsg::Input(vec![f]));
}

#[test]
fn hello_gets_a_seat_and_a_welcome() {
    let mut app = host_app();
    let peer = join(&mut app, "Tester");
    let control = peer.control();
    match control.as_slice() {
        [
            ServerMsg::Welcome(w),
            ServerMsg::Roster(names),
            ServerMsg::Load(load),
            ..,
        ] => {
            assert_eq!(w.player, PlayerId(1));
            assert_eq!(w.level, "test_yard.ron");
            assert_eq!(w.skill, Difficulty::Normal);
            assert_eq!(names, &["Host".to_string(), "Tester".to_string()]);
            assert_eq!(load.level, "test_yard.ron");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(player_ids(&mut app), [0, 1]);
    assert_eq!(seats(&app), 1);
}

fn refused(edit: impl FnOnce(&mut Hello)) -> Refusal {
    let mut app = host_app();
    let mut h = hello(&app, "Tester");
    edit(&mut h);
    let peer = greet(&mut app, h);
    assert_eq!(seats(&app), 0);
    assert_eq!(player_ids(&mut app), [0]);
    assert!(
        !peer.got.iter().any(|m| matches!(m, ServerMsg::Welcome(_))),
        "{:?}",
        peer.got
    );
    peer.refusal().expect("a refusal")
}

#[test]
fn wrong_assets_are_refused() {
    assert_eq!(refused(|h| h.assets ^= 1), Refusal::Assets);
}

#[test]
fn wrong_version_is_refused() {
    assert_eq!(
        refused(|h| h.game_version = "0.0.0-elsewhere".into()),
        Refusal::GameVersion
    );
}

#[test]
fn bad_name_is_refused() {
    assert_eq!(refused(|h| h.name = String::new()), Refusal::BadName);
    assert_eq!(refused(|h| h.name = "x".repeat(25)), Refusal::BadName);
}

#[test]
fn fifth_player_is_refused_full() {
    let mut app = host_app();
    let _a = join(&mut app, "A");
    let _b = join(&mut app, "B");
    let _c = join(&mut app, "C");
    assert_eq!(seats(&app), 3);
    let h = hello(&app, "D");
    let d = greet(&mut app, h);
    assert_eq!(d.refusal(), Some(Refusal::Full));
    assert_eq!(seats(&app), 3);
    assert_eq!(player_ids(&mut app), [0, 1, 2, 3]);
}

/// How far player 1 walks in 60 frames of `forward`.
fn walk(forward: f32) -> f32 {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    let start = pos(&mut app, PlayerId(1));
    for seq in 1..=60 {
        input(
            &mut peer,
            InputFrame {
                forward,
                angle: 0.3,
                ..frame(seq)
            },
        );
        pump(&mut app, &mut [&mut peer]);
    }
    let e = entity(&mut app, PlayerId(1));
    let held = app.world().get::<PendingInput>(e).unwrap().forward;
    assert!(held <= 1.0, "held forward {held}");
    assert!((angle(&mut app, PlayerId(1)) - 0.3).abs() < 1e-5);
    pos(&mut app, PlayerId(1))
        .truncate()
        .distance(start.truncate())
}

#[test]
fn input_moves_the_remote_player() {
    let d = walk(1.0);
    assert!(d > 1.0, "moved {d}");
}

#[test]
fn hostile_input_is_clamped_or_dropped() {
    let normal = walk(1.0);
    let hostile = walk(50.0);
    assert!(
        hostile <= normal * 1.05 + 0.05,
        "hostile {hostile} vs {normal}"
    );

    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    input(
        &mut peer,
        InputFrame {
            angle: 0.5,
            ..frame(100)
        },
    );
    pump_until(&mut app, &mut [&mut peer], "the frame", |app, _| {
        (angle(app, PlayerId(1)) - 0.5).abs() < 1e-5
    });
    // An older frame is ignored.
    input(
        &mut peer,
        InputFrame {
            angle: 2.0,
            ..frame(50)
        },
    );
    for _ in 0..10 {
        pump(&mut app, &mut [&mut peer]);
    }
    assert!((angle(&mut app, PlayerId(1)) - 0.5).abs() < 1e-5);
    // A frame of NaNs is garbage: the seat stays, the player is unharmed.
    let nan = InputFrame {
        forward: f32::NAN,
        angle: f32::NAN,
        pitch: f32::NAN,
        ..frame(200)
    };
    peer.client.send_raw(encode(&ClientMsg::Input(vec![nan])));
    peer.client.flush();
    for _ in 0..10 {
        pump(&mut app, &mut [&mut peer]);
    }
    assert_eq!(seats(&app), 1);
    assert_eq!(app.world().resource::<NetHost>().garbage, 1);
    assert!((angle(&mut app, PlayerId(1)) - 0.5).abs() < 1e-5);
    assert!(pos(&mut app, PlayerId(1)).is_finite());
    let e = entity(&mut app, PlayerId(1));
    assert_eq!(app.world().get::<PlayerVitals>(e).unwrap().0.health.hp, 100);
}

fn fires(events: &[(PlayerId, WeaponEvent)], id: PlayerId) -> usize {
    events
        .iter()
        .filter(|(p, e)| *p == id && matches!(e, WeaponEvent::Fire { .. }))
        .count()
}

#[test]
fn a_tap_fires_once() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    // Let the starting weapon come up.
    for _ in 0..60 {
        pump(&mut app, &mut [&mut peer]);
    }
    let mut fired = Vec::new();
    input(
        &mut peer,
        InputFrame {
            buttons: buttons::FIRE_PRESSED,
            ..frame(1)
        },
    );
    // No newer frame for a while: the tap must not repeat.
    for _ in 0..30 {
        fired.extend(pump(&mut app, &mut [&mut peer]));
    }
    for seq in 2..32 {
        input(&mut peer, frame(seq));
        fired.extend(pump(&mut app, &mut [&mut peer]));
    }
    assert_eq!(fires(&fired, PlayerId(1)), 1, "{fired:?}");
}

#[test]
fn snapshots_carry_the_seat() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    for _ in 0..30 {
        pump(&mut app, &mut [&mut peer]);
    }
    let snaps = peer.snapshots();
    assert!(snaps.len() >= 5, "{} snapshots", snaps.len());
    for (you, snap) in &snaps {
        assert_eq!(*you, PlayerId(1));
        assert_eq!(snap.players.len(), 2);
    }
    for pair in snaps.windows(2) {
        assert!(pair[1].1.tick >= pair[0].1.tick + 2, "{}", pair[1].1.tick);
    }
}

#[test]
fn leaver_closes_up_the_ids() {
    let mut app = host_app_with(|map| {
        map.actors.push(ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(12.0, 13.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
        });
    });
    let mut a = join(&mut app, "A");
    let mut b = join(&mut app, "B");
    // A carries the launcher.
    let ea = entity(&mut app, PlayerId(1));
    let defs = app.world().resource::<GameDefs>().0.clone();
    app.world_mut()
        .get_mut::<PlayerArsenal>(ea)
        .unwrap()
        .0
        .give_weapon(&defs, WeaponId::Rockets);
    input(
        &mut a,
        InputFrame {
            select: Some(WeaponId::Rockets),
            ..frame(1)
        },
    );
    for _ in 0..60 {
        pump(&mut app, &mut [&mut a, &mut b]);
    }
    input(
        &mut a,
        InputFrame {
            buttons: buttons::FIRE_PRESSED,
            ..frame(2)
        },
    );
    let a_shot = |app: &mut App| {
        app.world()
            .resource::<LevelCombat>()
            .0
            .projectiles
            .iter()
            .any(|p| p.owner == Shooter::Player(PlayerId(1)))
    };
    pump_until(&mut app, &mut [&mut a, &mut b], "A's rocket", |app, _| {
        a_shot(app)
    });
    {
        let combat = &mut app.world_mut().resource_mut::<LevelCombat>().0;
        combat.actors[0].target = Target::Player(PlayerId(1));
        combat.actors[0].grudge = 5.0;
    }
    a.client.disconnect();
    pump_until(&mut app, &mut [&mut b], "A gone", |app, _| {
        player_ids(app) == [0, 1]
    });
    let combat = &app.world().resource::<LevelCombat>().0;
    assert!(
        !combat
            .projectiles
            .iter()
            .any(|p| matches!(p.owner, Shooter::Player(_))),
        "no projectile is A's"
    );
    assert_eq!(combat.actors[0].target, Target::Player(PlayerId(0)));
    assert_eq!(combat.actors[0].grudge, 0.0);
    assert_eq!(seats(&app), 1);
    pump_until(&mut app, &mut [&mut b], "the new roster", |_, p| {
        p[0].got
            .iter()
            .any(|m| matches!(m, ServerMsg::Roster(n) if n == &["Host", "B"]))
    });
    b.got.clear();
    pump_until(&mut app, &mut [&mut b], "B's snapshot", |_, p| {
        !p[0].snapshots().is_empty()
    });
    assert_eq!(b.snapshots()[0].0, PlayerId(1));
    for _ in 0..120 {
        pump(&mut app, &mut [&mut b]);
    }
    assert_eq!(player_ids(&mut app), [0, 1]);
}

#[test]
fn restart_brings_remote_players_back() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    let first = peer.loads()[0].tick;
    for _ in 0..10 {
        pump(&mut app, &mut [&mut peer]);
    }
    restart_level(app.world_mut());
    pump_until(&mut app, &mut [&mut peer], "a second load", |_, p| {
        p[0].loads().len() >= 2
    });
    assert_eq!(player_ids(&mut app), [0, 1]);
    let second = peer.loads()[1].tick;
    assert!(second > first, "{second} after {first}");
    // The new run's snapshots are stamped after its Load, so a client drops none of them.
    let after_load = |p: &Peer| {
        let at = p
            .got
            .iter()
            .enumerate()
            .filter(|(_, m)| matches!(m, ServerMsg::Load(_)))
            .nth(1)
            .map(|(i, _)| i)
            .unwrap();
        p.got[at..].iter().find_map(|m| match m {
            ServerMsg::Snapshot { snap, .. } => Some(snap.tick),
            _ => None,
        })
    };
    pump_until(
        &mut app,
        &mut [&mut peer],
        "a snapshot after the load",
        |_, p| after_load(p[0]).is_some(),
    );
    let tick = after_load(&peer).unwrap();
    assert!(tick > second, "snapshot {tick} after load {second}");
}

#[test]
fn silent_connection_is_dropped() {
    let mut app = host_app();
    let mut peer = connect(&mut app);
    app.world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_millis(250));
    // Four seconds in, the connection still stands.
    const { assert!(16.0 * 0.25 < HELLO_TIMEOUT) };
    for _ in 0..16 {
        pump(&mut app, &mut [&mut peer]);
    }
    assert_eq!(peer.client.state(), ClientState::Connected);
    pump_until(&mut app, &mut [&mut peer], "the drop", |_, p| {
        matches!(p[0].client.state(), ClientState::Lost(_))
    });
    assert_eq!(seats(&app), 0);
}

/// A frame whose angle tells which one it is.
fn marked(seq: u32) -> InputFrame {
    InputFrame {
        angle: seq as f32 * 0.01,
        ..frame(seq)
    }
}

/// The seq the host last applied for seat 0.
fn ack(app: &App) -> u32 {
    app.world().resource::<NetHost>().seats[0].ack
}

/// Pumps `n` rounds and returns each new ack in turn (a frame per tick shows every seq).
fn applied(app: &mut App, peer: &mut Peer, n: usize) -> Vec<u32> {
    let mut acks: Vec<u32> = Vec::new();
    for _ in 0..n {
        pump(app, &mut [&mut *peer]);
        let a = ack(app);
        if a != 0 && acks.last() != Some(&a) {
            assert!(
                (angle(app, PlayerId(1)) - a as f32 * 0.01).abs() < 1e-5,
                "frame {a} is the one applied"
            );
            acks.push(a);
        }
    }
    acks
}

#[test]
fn redundant_frames_apply_once_each() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    // Four frames in all: not so deep that the queue drains past one.
    peer.send(&ClientMsg::Input((1..=3).map(marked).collect()));
    peer.send(&ClientMsg::Input((2..=4).map(marked).collect()));
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(applied(&mut app, &mut peer, 12), [1, 2, 3, 4]);
    pump_until(&mut app, &mut [&mut peer], "a snapshot acking 4", |_, p| {
        p[0].snapshots()
            .last()
            .is_some_and(|(_, s)| s.players[1].ack == 4)
    });
}

#[test]
fn lost_datagram_tap_survives() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    for _ in 0..60 {
        pump(&mut app, &mut [&mut peer]);
    }
    let tap = InputFrame {
        buttons: buttons::FIRE_PRESSED,
        ..frame(7)
    };
    let mut fired = Vec::new();
    peer.send(&ClientMsg::Input(vec![frame(5), frame(6)]));
    fired.extend(pump(&mut app, &mut [&mut peer]));
    // The datagram that carried 7 alone was lost; the next one repeats it.
    peer.send(&ClientMsg::Input(vec![frame(6), tap, frame(8)]));
    for _ in 0..30 {
        fired.extend(pump(&mut app, &mut [&mut peer]));
    }
    assert_eq!(fires(&fired, PlayerId(1)), 1, "{fired:?}");
    assert_eq!(ack(&app), 8);
}

#[test]
fn queue_is_capped() {
    let mut app = host_app();
    let mut peer = join(&mut app, "Tester");
    for start in (1..=20).step_by(4) {
        peer.send(&ClientMsg::Input((start..start + 4).map(marked).collect()));
    }
    std::thread::sleep(Duration::from_millis(10));
    let acks = applied(&mut app, &mut peer, 30);
    assert!(acks.len() <= MAX_INPUT_QUEUE, "{acks:?}");
    assert_eq!(acks.last(), Some(&20), "{acks:?}");
    assert!(
        acks[0] > 20 - MAX_INPUT_QUEUE as u32,
        "the oldest went: {acks:?}"
    );
    assert!(acks.windows(2).all(|w| w[1] > w[0]), "{acks:?}");
}

/// One-shot input the host applied to player 1 this tick, taken so nothing else acts on it.
#[derive(Resource, Default, Debug)]
struct Applied {
    /// The `buttons` bit of each tap applied.
    taps: Vec<u16>,
    selects: Vec<WeaponId>,
    cycle: i32,
}

fn record_applied(mut applied: ResMut<Applied>, mut q: Query<(&Player, &mut PendingInput)>) {
    let Some((_, mut input)) = q.iter_mut().find(|(p, _)| p.id == PlayerId(1)) else {
        return;
    };
    let input = &mut *input;
    for (bit, latch) in [
        (buttons::USE, &mut input.use_pressed),
        (buttons::FIRE_PRESSED, &mut input.fire_pressed),
        (buttons::RELOAD, &mut input.reload),
        (buttons::KICK, &mut input.kick),
        (buttons::MEDKIT, &mut input.use_medkit),
        (buttons::JETPACK, &mut input.toggle_jetpack),
        (buttons::NIGHT_VISION, &mut input.toggle_nv),
    ] {
        if std::mem::take(latch) {
            applied.taps.push(bit);
        }
    }
    if let Some(w) = input.select.take() {
        applied.selects.push(w);
    }
    applied.cycle += std::mem::take(&mut input.cycle);
}

fn depth(app: &App) -> usize {
    app.world().resource::<NetHost>().seats[0].queue.len()
}

#[test]
fn a_burst_drains_and_keeps_its_taps() {
    let mut app = host_app();
    app.init_resource::<Applied>().add_systems(
        FixedUpdate,
        record_applied.after(apply_remote_input).before(use_key),
    );
    let mut peer = join(&mut app, "Tester");
    // A burst of ten after a stall: past the cap, every one with a one-shot of its own.
    let taps = [
        buttons::USE,
        buttons::FIRE_PRESSED,
        buttons::RELOAD,
        buttons::KICK,
        buttons::MEDKIT,
        buttons::JETPACK,
        buttons::NIGHT_VISION,
    ];
    let burst: Vec<InputFrame> = (1..=10u32)
        .map(|seq| {
            let mut f = marked(seq);
            match seq {
                1 => f.select = Some(WeaponId::Shotgun),
                2..=8 => f.buttons = taps[seq as usize - 2],
                9 => f.select = Some(WeaponId::Pistol),
                _ => f.cycle = 1,
            }
            if seq == 1 {
                f.cycle = 1;
            }
            f
        })
        .collect();
    for part in burst.chunks(INPUT_REDUNDANCY) {
        peer.send(&ClientMsg::Input(part.to_vec()));
    }
    std::thread::sleep(Duration::from_millis(10));
    let mut deepest = Vec::new();
    for seq in 11..=40 {
        pump(&mut app, &mut [&mut peer]);
        deepest.push(depth(&app));
        // Then one frame a tick, as a client sends them.
        input(&mut peer, marked(seq));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        deepest[0],
        MAX_INPUT_QUEUE - 1,
        "the burst was queued: {deepest:?}"
    );
    let back = deepest
        .iter()
        .position(|&d| d <= QUEUE_SLACK)
        .expect("the queue drains");
    assert!(back < 20, "drained after {back} ticks: {deepest:?}");
    assert!(
        deepest[back..].iter().all(|&d| d <= QUEUE_SLACK),
        "stays drained: {deepest:?}"
    );
    let applied = std::mem::take(&mut *app.world_mut().resource_mut::<Applied>());
    let mut got = applied.taps.clone();
    got.sort();
    let mut want = taps.to_vec();
    want.sort();
    assert_eq!(got, want, "each tap once");
    assert_eq!(applied.selects, [WeaponId::Shotgun, WeaponId::Pistol]);
    assert_eq!(applied.cycle, 2);
    for _ in 0..5 {
        pump(&mut app, &mut [&mut peer]);
    }
    assert_eq!(ack(&app), 40);
}

#[test]
fn events_are_relayed_with_the_tick() {
    let mut app = host_app_with(|map| {
        map.actors.push(ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(12.0, 13.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
        });
    });
    let mut peer = join(&mut app, "Tester");
    for _ in 0..10 {
        pump(&mut app, &mut [&mut peer]);
    }
    {
        let combat = &mut app.world_mut().resource_mut::<LevelCombat>().0;
        let body = combat.actors[0].body;
        combat.pending_blasts.push(PendingBlast {
            fuse: 0.0,
            blast: Blast {
                center: body.pos + Vec3::Z,
                sector: body.sector,
                splash: SplashDef {
                    radius: 1.5,
                    damage: 1000,
                    self_scale: 1.0,
                },
                owner: Shooter::World,
            },
            via: Via::Direct,
        });
    }
    let killed = |p: &Peer| {
        p.got.iter().find_map(|m| match m {
            ServerMsg::Events(ev)
                if ev
                    .combat
                    .iter()
                    .any(|e| matches!(e, CombatEvent::ActorKilled { actor: 0, .. })) =>
            {
                Some(ev.tick)
            }
            _ => None,
        })
    };
    pump_until(&mut app, &mut [&mut peer], "the kill", |_, p| {
        killed(p[0]).is_some()
    });
    let tick = killed(&peer).unwrap();
    pump_until(&mut app, &mut [&mut peer], "a later snapshot", |_, p| {
        p[0].snapshots().iter().any(|(_, s)| s.tick >= tick)
    });
    let ticks: Vec<u32> = peer.snapshots().iter().map(|(_, s)| s.tick).collect();
    let before = ticks
        .iter()
        .copied()
        .filter(|t| *t < tick)
        .max()
        .expect("a snapshot before the kill");
    assert!(
        tick > before && tick <= before + SNAPSHOT_EVERY,
        "kill at {tick}, snapshots {ticks:?}"
    );
}

/// Puts player `id`'s body at `(x, y)`.
fn place(app: &mut App, id: PlayerId, x: f32, y: f32) {
    let e = entity(app, id);
    let map = app.world().resource::<CurrentMap>().0.clone();
    let mut b = app.world().get::<PlayerBody>(e).unwrap().0;
    let spawned = rr_core::collide::Body::spawn(&map, Vec2::new(x, y), b.radius, b.height).unwrap();
    b.pos = spawned.pos;
    b.sector = spawned.sector;
    b.vel = Vec3::ZERO;
    app.world_mut().get_mut::<PlayerBody>(e).unwrap().0 = b;
    app.world_mut().get_mut::<PrevFeet>(e).unwrap().0 = b.pos;
}

fn huds(p: &Peer) -> Vec<String> {
    p.got
        .iter()
        .filter_map(|m| match m {
            ServerMsg::Hud(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn hud_line_goes_to_its_player() {
    let mut app = host_app_with(|map| {
        map.items.push(Item {
            kind: ItemKind::Key(Key::Red),
            pos: Vec2::new(10.0, 13.0),
            skill: Difficulty::Easy,
        });
    });
    let mut a = join(&mut app, "A");
    let mut b = join(&mut app, "B");
    place(&mut app, PlayerId(1), 10.0, 13.0);
    pump_until(&mut app, &mut [&mut a, &mut b], "A's HUD line", |_, p| {
        !huds(p[0]).is_empty()
    });
    for _ in 0..20 {
        pump(&mut app, &mut [&mut a, &mut b]);
    }
    assert_eq!(huds(&a), ["Picked up the red keycard"]);
    assert!(huds(&b).is_empty(), "{:?}", huds(&b));
    assert_eq!(
        app.world()
            .resource::<rr_game::mechanics::HudMessage>()
            .text,
        ""
    );
}
