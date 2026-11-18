//! Joining a co-op game: a host app and a client app, both driven by hand, over real UDP on
//! 127.0.0.1.

use std::net::SocketAddr;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::actors::hurt;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::combat_room;
use rr_core::players::PlayerId;
use rr_core::rng::Rng;
use rr_core::snapshot::Snapshot;
use rr_game::combat::{CombatSimPlugin, FxQueue, GameDefs, LevelCombat, insert_defs};
use rr_game::flow::{FlowPlugin, LevelFile, LevelRuns, LevelSource, LocalSeat, restart_level};
use rr_game::level::{CurrentMap, load_map};
use rr_game::mechanics::{DirtySectors, LevelMechanics, MechanicsSimPlugin, insert_level};
use rr_game::net::NetMode;
use rr_game::net::client::stop_joining;
use rr_game::net::client::{
    NOT_UNDERSTOOD, NetClient, NetClientPlugin, NetNotice, client_apply, client_handle,
    start_joining,
};
use rr_game::net::host::{NetHost, NetHostPlugin, NetIdentity, start_hosting};
use rr_game::net::predict::{CorrectionOffset, InputHistory, reconcile};
use rr_game::player::{
    LocalPlayer, PendingInput, Player, PlayerBody, PlayerCamera, PlayerControlPlugin,
    PlayerSimPlugin, PrevFeet,
};
use rr_net::{InputFrame, Load, ServerMsg, buttons};

fn sim_plugins(app: &mut App) {
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        NetHostPlugin,
        NetClientPlugin,
    ));
}

fn host_app(level: &str) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    insert_level(&mut app, load_map(level), Difficulty::Normal);
    app.insert_resource(LevelFile(level.into()));
    insert_defs(&mut app, Defs::builtin());
    sim_plugins(&mut app);
    app.update();
    start_hosting(
        app.world_mut(),
        "127.0.0.1:0".parse().unwrap(),
        "Host".into(),
    )
    .expect("host");
    app
}

/// A client app on another level than any host's, not joined yet.
fn idle_client() -> App {
    client_with(|_| {})
}

/// [`idle_client`] with `extra` plugins added before the first update.
fn client_with(extra: impl FnOnce(&mut App)) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    extra(&mut app);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    insert_level(&mut app, combat_room(), Difficulty::Normal);
    app.insert_resource(LevelFile("combat_room".into()));
    insert_defs(&mut app, Defs::builtin());
    sim_plugins(&mut app);
    app.update();
    app
}

fn client_app(addr: SocketAddr) -> App {
    let mut app = idle_client();
    start_joining(app.world_mut(), addr, "Tester".into()).expect("client");
    app
}

fn host_addr(app: &App) -> SocketAddr {
    app.world().resource::<NetHost>().host.local_addr()
}

fn round(host: &mut App, client: &mut App) {
    host.update();
    host.world_mut().run_schedule(FixedUpdate);
    client.update();
    client.world_mut().run_schedule(FixedUpdate);
    std::thread::sleep(Duration::from_millis(1));
}

fn pump(host: &mut App, client: &mut App, n: usize) {
    for _ in 0..n {
        round(host, client);
    }
}

/// Pumps until `done` (at most 300 rounds); panics with `what` otherwise.
fn pump_until(
    host: &mut App,
    client: &mut App,
    what: &str,
    done: impl Fn(&mut App, &mut App) -> bool,
) {
    for _ in 0..300 {
        round(host, client);
        if done(host, client) {
            return;
        }
    }
    panic!("timed out waiting for {what}");
}

fn player_ids(app: &mut App) -> Vec<u8> {
    let mut q = app.world_mut().query::<&Player>();
    let mut ids: Vec<u8> = q.iter(app.world()).map(|p| p.id.0).collect();
    ids.sort();
    ids
}

fn local_id(app: &mut App) -> Option<PlayerId> {
    let mut q = app
        .world_mut()
        .query_filtered::<&Player, With<LocalPlayer>>();
    q.single(app.world()).ok().map(|p| p.id)
}

fn pos(app: &mut App, id: PlayerId) -> Vec3 {
    let mut q = app.world_mut().query::<(&Player, &PlayerBody)>();
    q.iter(app.world())
        .find(|(p, _)| p.id == id)
        .map(|(_, b)| b.0.pos)
        .unwrap_or_else(|| panic!("no player {id:?}"))
}

fn local_pos(app: &mut App) -> Vec3 {
    let mut q = app
        .world_mut()
        .query_filtered::<&PlayerBody, With<LocalPlayer>>();
    q.single(app.world()).unwrap().0.pos
}

fn notice(app: &App) -> Option<String> {
    app.world().resource::<NetNotice>().0.clone()
}

fn mode(app: &App) -> NetMode {
    app.world()
        .get_resource::<NetMode>()
        .copied()
        .unwrap_or_default()
}

/// A client that took the host's level and has applied a snapshot with both players.
fn joined(host: &mut App) -> App {
    let mut client = client_app(host_addr(host));
    wait_joined(host, &mut client);
    client
}

/// Pumps until `client` (joining `host`) has applied a snapshot with both players.
fn wait_joined(host: &mut App, client: &mut App) {
    pump_until(host, client, "the join", |_, c| {
        player_ids(c) == [0, 1]
            && c.world()
                .get_resource::<NetClient>()
                .is_some_and(|n| n.last_tick.is_some())
    });
}

#[test]
fn client_joins_and_plays_the_hosts_level() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    assert_eq!(
        *client.world().resource::<LocalSeat>(),
        LocalSeat(PlayerId(1))
    );
    assert_eq!(local_id(&mut client), Some(PlayerId(1)));
    assert_eq!(
        client.world().resource::<CurrentMap>().0.name,
        host.world().resource::<CurrentMap>().0.name
    );
    assert_eq!(
        client.world().resource::<LevelFile>().0,
        "test_yard.ron".to_string()
    );
    assert_eq!(player_ids(&mut client), [0, 1]);
    assert_eq!(mode(&client), NetMode::Client);
    let net = client.world().resource::<NetClient>();
    assert_eq!(net.roster, ["Host", "Tester"]);
    assert_eq!(net.welcomed.as_ref().map(|w| w.player), Some(PlayerId(1)));
}

#[test]
fn client_input_moves_its_player_on_both_sides() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    let start = pos(&mut host, PlayerId(1));
    for _ in 0..60 {
        set_forward(&mut client, 1.0);
        round(&mut host, &mut client);
    }
    let moved = pos(&mut host, PlayerId(1))
        .truncate()
        .distance(start.truncate());
    assert!(moved > 1.0, "moved {moved}");
    // Let the player come to rest, then one more snapshot lands.
    set_forward(&mut client, 0.0);
    pump(&mut host, &mut client, 60);
    let tick = client.world().resource::<NetClient>().last_tick;
    pump_until(&mut host, &mut client, "one more snapshot", |_, c| {
        c.world().resource::<NetClient>().last_tick > tick
    });
    let (h, c) = (pos(&mut host, PlayerId(1)), local_pos(&mut client));
    assert!(h.distance(c) < 0.05, "host {h} client {c}");
}

fn set_forward(client: &mut App, f: f32) {
    let mut q = client
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    q.single_mut(client.world_mut()).unwrap().forward = f;
}

/// Whether every remote player's `PrevFeet` and every actor's `prev_pos` is its current pose
/// (the local player is predicted and steps every tick).
fn at_rest(app: &mut App) -> bool {
    let mut q = app
        .world_mut()
        .query_filtered::<(&PlayerBody, &PrevFeet), Without<LocalPlayer>>();
    let players = q.iter(app.world()).all(|(b, p)| b.0.pos == p.0);
    let combat = &app.world().resource::<LevelCombat>().0;
    players && combat.actors.iter().all(|a| a.prev_pos == a.body.pos)
}

#[test]
fn interpolation_rests_between_snapshots() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    let (mut stepped, mut rested) = (0, 0);
    let mut applied_before = false;
    for _ in 0..120 {
        // The host walks: its player is remote on the client, so snapshots move it.
        set_forward(&mut host, 1.0);
        let before = client.world().resource::<NetClient>().last_tick;
        round(&mut host, &mut client);
        let applied = client.world().resource::<NetClient>().last_tick != before;
        if applied && !at_rest(&mut client) {
            stepped += 1;
        } else if !applied && applied_before {
            assert!(
                at_rest(&mut client),
                "a tick without a snapshot replays the step"
            );
            rested += 1;
        }
        applied_before = applied;
    }
    assert!(
        stepped > 0 && rested > 0,
        "stepped {stepped} rested {rested}"
    );
}

#[test]
fn host_kill_shows_on_the_client() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    assert!(client.world().resource::<LevelCombat>().0.actors[0].alive());
    {
        let defs = host.world().resource::<GameDefs>().0.clone();
        let combat = &mut host.world_mut().resource_mut::<LevelCombat>().0;
        let def = defs.enemy(combat.actors[0].kind).clone();
        hurt(&mut combat.actors[0], &def, 100_000, &mut Rng::new(1));
        assert!(!combat.actors[0].alive());
    }
    pump_until(&mut host, &mut client, "the kill", |_, c| {
        !c.world().resource::<LevelCombat>().0.actors[0].alive()
    });
}

#[test]
fn stale_snapshots_are_dropped() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    let last = client
        .world()
        .resource::<NetClient>()
        .last_tick
        .expect("a snapshot");
    // An old snapshot of a changed world: the actor moved and the player is elsewhere.
    let mut combat = host.world().resource::<LevelCombat>().0.clone();
    combat.actors[0].body.pos.x += 1.0;
    let mut players = Vec::new();
    {
        let mut q = host.world_mut().query::<(
            &Player,
            &PlayerBody,
            &rr_game::player::Look,
            &rr_game::combat::PlayerVitals,
            &rr_game::combat::PlayerArsenal,
            &rr_game::combat::PlayerInventory,
            &rr_game::player::Inventory,
        )>();
        for (p, body, look, vitals, arsenal, carried, inv) in q.iter(host.world()) {
            let mut body = body.0;
            body.pos.x += 0.5;
            players.push(rr_core::snapshot::PlayerSnap {
                id: p.id,
                body,
                angle: look.angle,
                pitch: look.pitch,
                vitals: vitals.0,
                arsenal: arsenal.0.clone(),
                inventory: carried.0,
                keys: inv.keys,
                ack: 0,
            });
        }
    }
    players.sort_by_key(|s| s.id);
    let snap = Snapshot::capture(
        last - 1,
        &combat,
        &host.world().resource::<LevelMechanics>().0,
        &host.world().resource::<CurrentMap>().0,
        &host.world().resource::<LevelSource>().0,
        players,
    );
    let before_actor = client.world().resource::<LevelCombat>().0.actors[0]
        .body
        .pos;
    let before_me = local_pos(&mut client);
    client.world_mut().resource_mut::<NetClient>().pending = Some((PlayerId(1), snap));
    client.world_mut().run_system_cached(client_apply).unwrap();
    assert_eq!(
        client.world().resource::<LevelCombat>().0.actors[0]
            .body
            .pos,
        before_actor
    );
    assert_eq!(local_pos(&mut client), before_me);
    assert_eq!(client.world().resource::<NetClient>().last_tick, Some(last));
}

#[test]
fn restart_reloads_the_client() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    let runs = client.world().resource::<LevelRuns>().0;
    restart_level(host.world_mut());
    pump_until(&mut host, &mut client, "the reload", |_, c| {
        c.world().resource::<LevelRuns>().0 > runs && player_ids(c) == [0, 1]
    });
    assert_eq!(local_id(&mut client), Some(PlayerId(1)));
    pump(&mut host, &mut client, 10);
    assert_eq!(player_ids(&mut client), [0, 1]);
    assert_eq!(local_id(&mut client), Some(PlayerId(1)));
}

#[test]
fn wrong_assets_show_a_notice() {
    let mut host = host_app("test_yard.ron");
    let mut client = client_app(host_addr(&host));
    client.world_mut().resource_mut::<NetIdentity>().assets ^= 1;
    pump_until(&mut host, &mut client, "the refusal", |_, c| {
        notice(c).is_some()
    });
    assert_eq!(
        notice(&client).as_deref(),
        Some("Your game files differ from the host's")
    );
    assert_eq!(mode(&client), NetMode::Single);
    assert!(!client.world().contains_resource::<NetClient>());
    assert_eq!(player_ids(&mut client), [0]);
}

#[test]
fn host_gone_shows_connection_lost() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    drop(host);
    client
        .world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_millis(250));
    for _ in 0..300 {
        client.update();
        client.world_mut().run_schedule(FixedUpdate);
        if notice(&client).is_some() {
            break;
        }
    }
    assert_eq!(notice(&client).as_deref(), Some("Connection lost"));
    assert_eq!(mode(&client), NetMode::Single);
    assert!(!client.world().contains_resource::<NetClient>());
    assert_eq!(
        *client.world().resource::<LocalSeat>(),
        LocalSeat(PlayerId(0))
    );
}

#[test]
fn undecodable_control_message_ends_the_session() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    {
        let mut net = host.world_mut().resource_mut::<NetHost>();
        let key = net.seats[0].client;
        net.host.send_raw(key, vec![0xff; 40]);
    }
    pump_until(&mut host, &mut client, "the notice", |_, c| {
        notice(c).is_some()
    });
    assert_eq!(notice(&client).as_deref(), Some(NOT_UNDERSTOOD));
    assert_eq!(mode(&client), NetMode::Single);
    assert!(!client.world().contains_resource::<NetClient>());
    assert_eq!(player_ids(&mut client), [0]);
}

#[test]
fn only_a_level_file_can_be_hosted() {
    let mut app = idle_client();
    for bad in ["combat_room", "../x.ron", "Yard.ron"] {
        app.insert_resource(LevelFile(bad.into()));
        let err = start_hosting(app.world_mut(), "127.0.0.1:0".parse().unwrap(), "H".into())
            .expect_err(bad);
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{bad}");
        assert_eq!(mode(&app), NetMode::Single, "{bad}");
        assert!(!app.world().contains_resource::<NetHost>(), "{bad}");
    }
}

#[test]
fn bad_load_level_is_refused_locally() {
    let mut client = idle_client();
    // Nobody listens there; the handling under test needs no host.
    start_joining(
        client.world_mut(),
        "127.0.0.1:9".parse().unwrap(),
        "Tester".into(),
    )
    .expect("client");
    let map = client.world().resource::<CurrentMap>().0.name.clone();
    client_handle(
        client.world_mut(),
        ServerMsg::Load(Load {
            level: "missing_level.ron".into(),
            skill: Difficulty::Normal,
            tick: 5,
        }),
    );
    assert_eq!(
        notice(&client).as_deref(),
        Some("The host's level is not on this machine")
    );
    assert_eq!(mode(&client), NetMode::Single);
    assert!(!client.world().contains_resource::<NetClient>());
    assert_eq!(client.world().resource::<CurrentMap>().0.name, map);
}

#[test]
fn glass_broken_on_host_remeshes_on_client() {
    let mut host = host_app("arsenal_depot.ron");
    let mut client = joined(&mut host);
    let wall = {
        let map = &host.world().resource::<CurrentMap>().0;
        (0..map.walls.len())
            .find(|&w| map.walls[w].glass)
            .expect("arsenal_depot has glass")
    };
    let (side, back) = {
        let map = &host.world().resource::<CurrentMap>().0;
        (map.walls[wall].sector, map.walls[wall].next_sector.unwrap())
    };
    {
        let world = host.world_mut();
        world.resource_scope(|world, mut combat: Mut<LevelCombat>| {
            let mut map = world.resource_mut::<CurrentMap>();
            assert!(!combat.0.destruct.break_glass(&mut map.0, wall).is_empty());
        });
    }
    client.world_mut().resource_mut::<DirtySectors>().0.clear();
    let panes = |c: &App| -> Vec<usize> {
        c.world()
            .resource::<FxQueue>()
            .combat
            .iter()
            .filter_map(|e| match e {
                rr_core::combat::CombatEvent::GlassBroken { wall, .. } => Some(*wall),
                _ => None,
            })
            .collect()
    };
    pump_until(&mut host, &mut client, "the glass", |_, c| {
        !panes(c).is_empty()
    });
    // One event for the pane, not one per side.
    let twin = host.world().resource::<CurrentMap>().0.walls[wall]
        .next_wall
        .unwrap();
    assert_eq!(panes(&client), [wall.min(twin)]);
    let dirty = &client.world().resource::<DirtySectors>().0;
    assert!(dirty.contains(&side) && dirty.contains(&back), "{dirty:?}");
    assert!(!client.world().resource::<CurrentMap>().0.walls[wall].glass);
}

/// One client tick alone: no host round, so nothing can have come back from it.
fn client_tick(client: &mut App) {
    client.update();
    client.world_mut().run_schedule(FixedUpdate);
}

#[test]
fn own_movement_is_immediate() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    let start = local_pos(&mut client);
    set_forward(&mut client, 1.0);
    client_tick(&mut client);
    let moved = local_pos(&mut client).truncate().distance(start.truncate());
    assert!(moved > 0.0, "moved {moved}");
}

#[test]
fn prediction_matches_the_host() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    for _ in 0..60 {
        set_forward(&mut client, 1.0);
        round(&mut host, &mut client);
    }
    set_forward(&mut client, 0.0);
    pump(&mut host, &mut client, 30);
    let (h, c) = (pos(&mut host, PlayerId(1)), local_pos(&mut client));
    assert!(h.distance(c) < 0.05, "host {h} client {c}");
}

/// Moves the host's player 1 by `dx` metres east; returns its new feet.
fn teleport(host: &mut App, dx: f32) -> Vec3 {
    let map = host.world().resource::<CurrentMap>().0.clone();
    let mut q = host.world_mut().query::<(&Player, &mut PlayerBody)>();
    let (_, mut body) = q
        .iter_mut(host.world_mut())
        .find(|(p, _)| p.id == PlayerId(1))
        .unwrap();
    body.0.pos.x += dx;
    body.0.sector = map
        .find_sector(body.0.pos.truncate(), Some(body.0.sector))
        .expect("the new spot is in the level");
    body.0.pos
}

#[test]
fn correction_is_applied() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    let spot = teleport(&mut host, 3.0);
    let tick = host.world().resource::<NetHost>().tick;
    pump_until(
        &mut host,
        &mut client,
        "a snapshot after the move",
        |_, c| {
            c.world()
                .resource::<NetClient>()
                .last_tick
                .is_some_and(|t| t > tick)
        },
    );
    let c = local_pos(&mut client);
    let off = c.truncate().distance(spot.truncate());
    assert!(off < 0.05, "client {c} spot {spot}");
}

/// The local player's `Fire` events in the client's [`FxQueue`] now.
fn own_fires(client: &mut App) -> usize {
    let me = local_id(client).unwrap();
    client
        .world()
        .resource::<FxQueue>()
        .weapon
        .iter()
        .filter(|(p, e)| *p == me && matches!(e, rr_core::weapons::WeaponEvent::Fire { .. }))
        .count()
}

#[test]
fn own_shot_is_immediate_and_single() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 30);
    {
        let mut q = client
            .world_mut()
            .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
        q.single_mut(client.world_mut()).unwrap().fire_pressed = true;
    }
    client_tick(&mut client);
    assert_eq!(own_fires(&mut client), 1, "the shot on the press tick");
    let mut later = 0;
    for _ in 0..60 {
        round(&mut host, &mut client);
        later += own_fires(&mut client);
    }
    assert_eq!(later, 0, "the shot shows once");
}

#[test]
fn replay_discards_events() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 30);
    let snap = {
        let world = client.world_mut();
        let mut q = world.query_filtered::<(
            &Player,
            &PlayerBody,
            &rr_game::player::Look,
            &rr_game::combat::PlayerVitals,
            &rr_game::combat::PlayerArsenal,
            &rr_game::combat::PlayerInventory,
            &rr_game::player::Inventory,
        ), With<LocalPlayer>>();
        let (p, body, look, vitals, arsenal, carried, inv) = q.single(world).unwrap();
        rr_core::snapshot::PlayerSnap {
            id: p.id,
            body: body.0,
            angle: look.angle,
            pitch: look.pitch,
            vitals: vitals.0,
            arsenal: arsenal.0.clone(),
            inventory: carried.0,
            keys: inv.keys,
            ack: 1000,
        }
    };
    let frames = (1001..=1010).map(|seq| InputFrame {
        seq,
        forward: 1.0,
        strafe: 0.0,
        angle: snap.angle,
        pitch: 0.0,
        buttons: buttons::FIRE | buttons::FIRE_PRESSED,
        select: None,
        cycle: 0,
    });
    client.world_mut().resource_mut::<InputHistory>().0 = frames.collect();
    {
        let mut fx = client.world_mut().resource_mut::<FxQueue>();
        fx.combat.clear();
        fx.weapon.clear();
        fx.mech.clear();
    }
    reconcile(client.world_mut(), &snap);
    let fx = client.world().resource::<FxQueue>();
    assert!(
        fx.combat.is_empty() && fx.weapon.is_empty() && fx.mech.is_empty(),
        "replay raised events"
    );
    assert_eq!(client.world().resource::<InputHistory>().0.len(), 10);
    let moved = local_pos(&mut client)
        .truncate()
        .distance(snap.body.pos.truncate());
    assert!(moved > 0.0, "the frames were replayed");
}

#[test]
fn rejoining_starts_a_fresh_input_stream() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    // Leave while walking, with frames the host never acknowledged.
    for _ in 0..60 {
        set_forward(&mut client, 1.0);
        round(&mut host, &mut client);
    }
    stop_joining(client.world_mut());
    pump_until(
        &mut host,
        &mut client,
        "the host to drop the seat",
        |h, _| player_ids(h) == [0],
    );
    start_joining(client.world_mut(), host_addr(&host), "Tester".into()).expect("rejoin");
    wait_joined(&mut host, &mut client);
    // No frame of the old session is sent or replayed (well inside `INPUT_HISTORY`).
    let seq = client.world().resource::<NetClient>().seq;
    let history = &client.world().resource::<InputHistory>().0;
    assert!(
        history.iter().all(|f| f.seq <= seq),
        "old frames {:?} past seq {seq}",
        history.iter().map(|f| f.seq).collect::<Vec<_>>()
    );
    let start = pos(&mut host, PlayerId(1));
    for _ in 0..30 {
        set_forward(&mut client, 1.0);
        round(&mut host, &mut client);
    }
    set_forward(&mut client, 0.0);
    pump(&mut host, &mut client, 30);
    let h = pos(&mut host, PlayerId(1));
    let moved = h.truncate().distance(start.truncate());
    assert!(moved > 0.5, "the host's player moved {moved}");
    let c = local_pos(&mut client);
    assert!(h.distance(c) < 0.05, "host {h} client {c}");
}

#[test]
fn steady_walking_needs_no_corrections() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    let mut checked = 0;
    for _ in 0..120 {
        set_forward(&mut client, 1.0);
        let before = client.world().resource::<NetClient>().last_tick;
        round(&mut host, &mut client);
        if client.world().resource::<NetClient>().last_tick != before {
            let last = client.world().resource::<CorrectionOffset>().last;
            assert!(last < 0.01, "a correction of {last} m");
            checked += 1;
        }
    }
    assert!(checked > 30, "only {checked} snapshots");
}

/// A joined client that also runs the camera (scripted: no keyboard or mouse), both apps on a
/// 60 Hz frame clock so frames have a duration and each runs one fixed tick.
fn camera_pair() -> (App, App) {
    let mut host = host_app("test_yard.ron");
    let mut client = client_with(|app| {
        app.add_plugins(PlayerControlPlugin { scripted: true });
    });
    start_joining(client.world_mut(), host_addr(&host), "Tester".into()).expect("client");
    wait_joined(&mut host, &mut client);
    let frame = || TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0));
    host.insert_resource(frame());
    client.insert_resource(frame());
    for _ in 0..10 {
        frame_round(&mut host, &mut client);
    }
    (host, client)
}

/// One frame of both apps on the frame clock (FixedUpdate runs from the clock).
fn frame_round(host: &mut App, client: &mut App) {
    host.update();
    client.update();
    std::thread::sleep(Duration::from_millis(1));
}

fn camera_pos(client: &mut App) -> Vec3 {
    let mut q = client
        .world_mut()
        .query_filtered::<&Transform, With<PlayerCamera>>();
    q.single(client.world()).unwrap().translation
}

/// Teleports the host's player 1 by `dx` and runs frames until its snapshot is in; returns the
/// camera before the move and on the frame the snapshot landed.
fn camera_on_correction(host: &mut App, client: &mut App, dx: f32) -> (Vec3, Vec3) {
    let before = camera_pos(client);
    teleport(host, dx);
    let tick = host.world().resource::<NetHost>().tick;
    for _ in 0..300 {
        frame_round(host, client);
        if client
            .world()
            .resource::<NetClient>()
            .last_tick
            .is_some_and(|t| t > tick)
        {
            return (before, camera_pos(client));
        }
    }
    panic!("no snapshot after the move");
}

#[test]
fn a_small_correction_slides_the_camera() {
    let (mut host, mut client) = camera_pair();
    let (before, first) = camera_on_correction(&mut host, &mut client, 0.3);
    assert!(
        first.distance(before) < 0.05,
        "the camera jumped {}",
        first.distance(before)
    );
    let mut moved = 0.0;
    for _ in 0..10 {
        frame_round(&mut host, &mut client);
        moved = camera_pos(&mut client).distance(before);
    }
    assert!((moved - 0.3).abs() < 0.01, "the camera moved {moved}");
}

#[test]
fn a_teleport_snaps_the_camera() {
    let (mut host, mut client) = camera_pair();
    let (before, first) = camera_on_correction(&mut host, &mut client, 3.0);
    let jump = first.distance(before);
    assert!((jump - 3.0).abs() < 0.05, "the camera jumped {jump}");
}
