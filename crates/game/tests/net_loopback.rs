//! Joining a co-op game: a host app and a client app, both driven by hand, over real UDP on
//! 127.0.0.1.

use std::net::SocketAddr;
use std::time::Duration;

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::world_serialization::WorldAsset;
use rr_core::actors::hurt;
use rr_core::collide::Body;
use rr_core::combat::{CombatEvent, PendingBlast, Via};
use rr_core::defs::Defs;
use rr_core::defs::{SplashDef, WeaponId};
use rr_core::difficulty::Difficulty;
use rr_core::explosion::Blast;
use rr_core::fixtures::combat_room;
use rr_core::map::SwitchAction;
use rr_core::players::PlayerId;
use rr_core::projectile::{Projectile, Shooter, Targets};
use rr_core::rng::Rng;
use rr_core::snapshot::Snapshot;
use rr_core::weapons::WeaponEvent;
use rr_game::actors::{ActorVisual, ActorVisualsPlugin, Spark};
use rr_game::breakables::BreakablesPlugin;
use rr_game::combat::PlayerVitals;
use rr_game::combat::{CombatSimPlugin, FxQueue, GameDefs, LevelCombat, insert_defs};
use rr_game::coop::CoopPlugin;
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin, Stats};
use rr_game::flow::{
    AdvanceRequested, FlowPlugin, LevelFile, LevelRuns, LevelSource, LocalSeat, PausedFrom,
    PlayState, restart_level,
};
use rr_game::hud::HudPlugin;
use rr_game::level::{CurrentMap, LevelLight, LevelRenderPlugin, load_map};
use rr_game::mechanics::HudMessage;
use rr_game::mechanics::{DirtySectors, LevelMechanics, MechanicsSimPlugin, insert_level};
use rr_game::menu::{MenuAction, MenuInput, MenuPlugin, MenuScreen, Screen};
use rr_game::models::ModelsPlugin;
use rr_game::net::NetMode;
use rr_game::net::client::stop_joining;
use rr_game::net::client::{
    HostRun, NOT_UNDERSTOOD, NetClient, NetClientPlugin, NetNotice, client_handle, start_joining,
};
use rr_game::net::host::{NetHost, NetHostPlugin, NetIdentity, start_hosting};
use rr_game::net::interp::{INTERP_DELAY, Timeline};
use rr_game::net::predict::{CorrectionOffset, InputHistory, reconcile};
use rr_game::player::{
    LocalPlayer, Look, PendingInput, Player, PlayerBody, PlayerCamera, PlayerControlPlugin,
    PlayerSimPlugin, PrevFeet,
};
use rr_net::{InputFrame, Load, ServerMsg, TickEvents, buttons};

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

/// The tick of the newest snapshot the client holds: it corrects the local player on arrival,
/// [`INTERP_DELAY`] ticks before the rest of the world is drawn at it.
fn newest_held(app: &App) -> Option<u32> {
    app.world()
        .resource::<Timeline>()
        .snaps
        .back()
        .map(|(_, s)| s.tick)
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
fn interpolation_rests_without_a_next_snapshot() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    for _ in 0..30 {
        // The host walks: its player is remote on the client, so snapshots move it.
        set_forward(&mut host, 1.0);
        round(&mut host, &mut client);
    }
    // The host falls silent: the client draws past every snapshot it holds.
    for _ in 0..(2 * INTERP_DELAY as usize + 4) {
        client_tick(&mut client);
    }
    assert!(
        at_rest(&mut client),
        "a tick without a next snapshot replays the step"
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
    let held = |c: &App| -> Vec<u32> {
        let t = c.world().resource::<Timeline>();
        t.snaps.iter().map(|(_, s)| s.tick).collect()
    };
    let before = held(&client);
    client_handle(
        client.world_mut(),
        ServerMsg::Snapshot {
            you: PlayerId(1),
            snap,
        },
    );
    assert_eq!(held(&client), before, "an old snapshot is not held");
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
    // A blast at the pane: the host's combat breaks it and relays `GlassBroken`.
    let center = {
        let map = &host.world().resource::<CurrentMap>().0;
        let w = &map.walls[wall];
        let mid = (w.a + w.b) / 2.0;
        let inward = (w.b - w.a).perp().normalize() * 0.3;
        let at = [mid + inward, mid - inward]
            .into_iter()
            .find(|p| map.find_sector(*p, None) == Some(side))
            .expect("a spot beside the pane");
        at.extend(map.sectors[side].floor_z + 1.0)
    };
    blast(&mut host, center, side, 2.0, 0);
    client.world_mut().resource_mut::<DirtySectors>().0.clear();
    let panes = |c: &App| -> Vec<usize> {
        c.world()
            .resource::<FxQueue>()
            .combat
            .iter()
            .filter_map(|e| match e {
                CombatEvent::GlassBroken { wall, .. } => Some(*wall),
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
    // The blast may break a neighbouring pane too; each pane is reported once.
    let got = panes(&client);
    assert!(got.contains(&wall.min(twin)), "{got:?}");
    assert!(!got.contains(&wall.max(twin)), "{got:?}");
    let mut once = got.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), got.len(), "{got:?}");
    let dirty = &client.world().resource::<DirtySectors>().0;
    assert!(dirty.contains(&side) && dirty.contains(&back), "{dirty:?}");
    assert!(!client.world().resource::<CurrentMap>().0.walls[wall].glass);
}

/// A client that also draws the level, its lights and the actors (no GPU, no glTF loader).
fn drawing_client() -> App {
    client_with(|app| {
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<WorldAsset>()
            .init_asset::<Gltf>()
            .init_asset::<AnimationClip>()
            .init_asset::<AnimationGraph>()
            .add_plugins((
                ModelsPlugin,
                LevelRenderPlugin,
                BreakablesPlugin,
                ActorVisualsPlugin,
            ));
    })
}

#[test]
fn late_joiner_sees_minions_and_broken_lights() {
    let mut host = host_app("arsenal_depot.ron");
    let light = {
        let map = &host.world().resource::<CurrentMap>().0;
        (0..map.lights.len())
            .find(|&i| map.lights[i].breakable)
            .expect("arsenal_depot has a breakable light")
    };
    // Before anyone joins: a light is shot out and an actor joins play (as a boss's minion).
    let minion = {
        let world = host.world_mut();
        world.resource_scope(|world, mut combat: Mut<LevelCombat>| {
            let map = &world.resource::<CurrentMap>().0;
            assert!(combat.0.destruct.break_light(map, light));
            let a = combat.0.actors[0].clone();
            combat.0.actors.push(a);
            combat.0.actors.len() - 1
        })
    };
    pump_host(&mut host, 5);
    let mut client = drawing_client();
    start_joining(client.world_mut(), host_addr(&host), "Tester".into()).expect("client");
    wait_joined(&mut host, &mut client);
    pump(&mut host, &mut client, 5);
    let shown: Vec<usize> = {
        let mut q = client.world_mut().query::<&ActorVisual>();
        q.iter(client.world()).map(|v| v.0).collect()
    };
    assert!(
        shown.contains(&minion),
        "no visual for actor {minion}: {shown:?}"
    );
    assert_eq!(
        shown.iter().filter(|&&i| i == minion).count(),
        1,
        "one visual each"
    );
    let lights: Vec<usize> = {
        let mut q = client.world_mut().query::<&LevelLight>();
        q.iter(client.world()).map(|l| l.0).collect()
    };
    assert!(
        !lights.contains(&light),
        "light {light} still lit: {lights:?}"
    );
    let all = host.world().resource::<CurrentMap>().0.lights.len();
    assert_eq!(lights.len(), all - 1, "the others stay lit: {lights:?}");
    assert!(
        client
            .world()
            .resource::<LevelCombat>()
            .0
            .destruct
            .light_broken(light),
        "the snapshot broke it"
    );
    let sparks = {
        let mut q = client.world_mut().query_filtered::<(), With<Spark>>();
        q.iter(client.world()).count()
    };
    assert_eq!(sparks, 0, "an old break throws no sparks");
}

#[test]
fn a_spawn_right_after_a_load_shows_once() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    restart_level(host.world_mut());
    // A minion in the new run's first tick: its own event and the first snapshot both carry it.
    // (After the frame, which empties `FxQueue`, before the tick that relays it.)
    host.update();
    let minion = {
        let world = host.world_mut();
        let mut combat = world.resource_mut::<LevelCombat>();
        let a = combat.0.actors[0].clone();
        combat.0.actors.push(a);
        let i = combat.0.actors.len() - 1;
        world
            .resource_mut::<FxQueue>()
            .combat
            .push(CombatEvent::ActorSpawned(i));
        i
    };
    host.world_mut().run_schedule(FixedUpdate);
    client.update();
    client.world_mut().run_schedule(FixedUpdate);
    let mut seen = 0;
    for _ in 0..60 {
        round(&mut host, &mut client);
        seen += client
            .world()
            .resource::<FxQueue>()
            .combat
            .iter()
            .filter(|e| **e == CombatEvent::ActorSpawned(minion))
            .count();
    }
    assert_eq!(seen, 1);
    assert_eq!(
        client.world().resource::<LevelCombat>().0.actors.len(),
        minion + 1
    );
}

/// `n` host ticks with no client.
fn pump_host(host: &mut App, n: usize) {
    for _ in 0..n {
        host.update();
        host.world_mut().run_schedule(FixedUpdate);
    }
}

/// Queues a blast on the host's combat, going off on its next tick.
fn blast(host: &mut App, center: Vec3, sector: usize, radius: f32, damage: i32) {
    let combat = &mut host.world_mut().resource_mut::<LevelCombat>().0;
    combat.pending_blasts.push(PendingBlast {
        fuse: 0.0,
        blast: Blast {
            center,
            sector,
            splash: SplashDef {
                radius,
                damage,
                self_scale: 1.0,
            },
            owner: Shooter::World,
        },
        via: Via::Direct,
    });
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
        let before = newest_held(&client);
        round(&mut host, &mut client);
        if newest_held(&client) != before {
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
        if newest_held(client).is_some_and(|t| t > tick) {
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

/// Turns the host's own player to `angle` (radians, 0 = east).
fn face(host: &mut App, angle: f32) {
    let mut q = host
        .world_mut()
        .query_filtered::<&mut rr_game::player::Look, With<LocalPlayer>>();
    q.single_mut(host.world_mut()).unwrap().angle = angle;
}

#[test]
fn remote_players_move_smoothly() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    // East-north-east: clear of the pillar and of player 1, south-east of the start.
    face(&mut host, 0.3);
    for _ in 0..20 {
        set_forward(&mut host, 1.0);
        round(&mut host, &mut client);
    }
    let mut last = pos(&mut client, PlayerId(0)).x;
    for i in 0..20 {
        set_forward(&mut host, 1.0);
        round(&mut host, &mut client);
        let x = pos(&mut client, PlayerId(0)).x;
        assert!(x > last, "tick {i}: x {x} after {last}");
        last = x;
    }
}

#[test]
fn world_is_drawn_behind_by_the_delay() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 60);
    let t = client.world().resource::<Timeline>();
    let render = i64::from(t.render.expect("a render tick"));
    let newest = i64::from(newest_held(&client).unwrap());
    let behind = newest - render;
    assert!(
        (behind - i64::from(INTERP_DELAY)).abs() <= 2,
        "render {render} newest {newest}"
    );
}

/// How far the client's render tick is from `newest - INTERP_DELAY` (positive: ahead).
fn drift(client: &App) -> i64 {
    let t = client.world().resource::<Timeline>();
    let render = i64::from(t.render.expect("a render tick"));
    render - (i64::from(newest_held(client).unwrap()) - i64::from(INTERP_DELAY))
}

/// Runs 30 rounds after a stall and checks the render tick came back within a tick of its
/// target without a jump.
fn eases_back(host: &mut App, client: &mut App, resyncs: u32) {
    let mut drifts = Vec::new();
    for _ in 0..30 {
        round(host, client);
        drifts.push(drift(client));
    }
    assert!(
        drifts[drifts.len() - 10..].iter().all(|d| d.abs() <= 1),
        "{drifts:?}"
    );
    assert_eq!(
        client.world().resource::<Timeline>().resyncs,
        resyncs,
        "eased, not jumped: {drifts:?}"
    );
}

#[test]
fn render_eases_back_after_a_host_stall() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 30);
    let resyncs = client.world().resource::<Timeline>().resyncs;
    // The host skips four ticks: no snapshots, the client's render tick runs on.
    for _ in 0..4 {
        client_tick(&mut client);
        std::thread::sleep(Duration::from_millis(1));
    }
    eases_back(&mut host, &mut client, resyncs);
}

#[test]
fn render_eases_back_after_a_client_stall() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 30);
    let resyncs = client.world().resource::<Timeline>().resyncs;
    // The client skips four ticks: the host's snapshots pile up meanwhile.
    for _ in 0..4 {
        host.update();
        host.world_mut().run_schedule(FixedUpdate);
        std::thread::sleep(Duration::from_millis(1));
    }
    eases_back(&mut host, &mut client, resyncs);
}

fn explosions(client: &App) -> usize {
    client
        .world()
        .resource::<FxQueue>()
        .combat
        .iter()
        .filter(|e| matches!(e, CombatEvent::Explosion { .. }))
        .count()
}

#[test]
fn explosion_reaches_the_client_once() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    pump(&mut host, &mut client, 10);
    const ROCKET: u32 = 999_999;
    {
        let start = pos(&mut host, PlayerId(0));
        let sector = {
            let mut q = host.world_mut().query::<(&Player, &PlayerBody)>();
            q.iter(host.world())
                .find(|(p, _)| p.id == PlayerId(0))
                .unwrap()
                .1
                .0
                .sector
        };
        let at = start + Vec3::Z;
        // North, into the courtyard's wall.
        host.world_mut()
            .resource_mut::<LevelCombat>()
            .0
            .projectiles
            .push(Projectile {
                id: ROCKET,
                pos: at,
                prev: at,
                vel: Vec3::new(0.0, 20.0, 0.0),
                sector,
                radius: 0.1,
                damage: 50,
                owner: Shooter::Player(PlayerId(0)),
                targets: Targets::Actors,
                life: 5.0,
                gravity: 0.0,
                bounce: None,
                remote: false,
                splash: Some(SplashDef {
                    radius: 2.0,
                    damage: 50,
                    self_scale: 1.0,
                }),
                resting: false,
            });
    }
    let held = |c: &App| {
        c.world()
            .resource::<LevelCombat>()
            .0
            .projectiles
            .iter()
            .any(|p| p.id == ROCKET)
    };
    let (mut seen, mut booms) = (false, 0);
    for _ in 0..120 {
        round(&mut host, &mut client);
        seen |= held(&client);
        let n = explosions(&client);
        if n > 0 {
            assert!(
                !held(&client),
                "the blast came before the rocket's last snapshot"
            );
        }
        booms += n;
    }
    assert!(seen, "the rocket never reached the client");
    assert_eq!(booms, 1);
}

#[test]
fn events_follow_their_snapshot() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    let body = host.world().resource::<LevelCombat>().0.actors[0].body;
    blast(&mut host, body.pos + Vec3::Z, body.sector, 1.5, 1000);
    let mut seen = false;
    for _ in 0..120 {
        round(&mut host, &mut client);
        let killed = client
            .world()
            .resource::<FxQueue>()
            .combat
            .iter()
            .any(|e| matches!(e, CombatEvent::ActorKilled { actor: 0, .. }));
        if killed {
            assert!(
                !client.world().resource::<LevelCombat>().0.actors[0].alive(),
                "the kill came before its snapshot"
            );
            seen = true;
        }
    }
    assert!(seen, "the kill never reached the client");
}

/// Hands the client `ev` as if the host sent it.
fn inject(client: &mut App, ev: TickEvents) {
    client_handle(client.world_mut(), ServerMsg::Events(ev));
}

fn hurt_event(actor: usize, amount: i32) -> CombatEvent {
    CombatEvent::ActorHurt { actor, amount }
}

#[test]
fn bad_event_is_dropped() {
    let mut host = host_app("combat_arena.ron");
    let mut client = joined(&mut host);
    let tick = newest_held(&client).unwrap() + 1;
    inject(
        &mut client,
        TickEvents {
            tick,
            combat: vec![hurt_event(9999, 5), hurt_event(0, 7)],
            weapon: vec![],
            mech: vec![],
        },
    );
    let has = |c: &App, e: &CombatEvent| c.world().resource::<FxQueue>().combat.contains(e);
    let mut good = false;
    for _ in 0..60 {
        round(&mut host, &mut client);
        assert!(!has(&client, &hurt_event(9999, 5)), "an event for no actor");
        good |= has(&client, &hurt_event(0, 7));
    }
    assert!(good, "the fitting event of the same tick was released");
}

#[test]
fn old_run_events_are_dropped() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    let runs = client.world().resource::<LevelRuns>().0;
    restart_level(host.world_mut());
    pump_until(&mut host, &mut client, "the reload", |_, c| {
        c.world().resource::<LevelRuns>().0 > runs
    });
    let floor = client.world().resource::<Timeline>().floor;
    let boom = |radius: f32| CombatEvent::Explosion {
        point: Vec3::new(3.0, 3.0, 1.0),
        radius,
    };
    for (tick, radius) in [(floor - 1, 1.25), (floor, 1.5), (floor + 1, 1.75)] {
        inject(
            &mut client,
            TickEvents {
                tick,
                combat: vec![boom(radius)],
                weapon: vec![],
                mech: vec![],
            },
        );
    }
    let has = |c: &App, e: &CombatEvent| c.world().resource::<FxQueue>().combat.contains(e);
    let mut new_run = false;
    for _ in 0..60 {
        round(&mut host, &mut client);
        assert!(!has(&client, &boom(1.25)) && !has(&client, &boom(1.5)));
        new_run |= has(&client, &boom(1.75));
    }
    assert!(new_run, "an event of the new run was released");
}

#[test]
fn hud_line_is_shown() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    {
        let mut net = host.world_mut().resource_mut::<NetHost>();
        let key = net.seats[0].client;
        net.host.send(key, &ServerMsg::Hud("Got it".into()));
    }
    pump_until(&mut host, &mut client, "the HUD line", |_, c| {
        c.world().resource::<HudMessage>().text == "Got it"
    });
}

#[test]
fn own_weapon_events_not_doubled() {
    let mut host = host_app("test_yard.ron");
    let mut client = joined(&mut host);
    let me = local_id(&mut client).unwrap();
    let fire = || WeaponEvent::Fire {
        weapon: WeaponId::Pistol,
        dirs: vec![Vec3::X],
    };
    let tick = newest_held(&client).unwrap() + 1;
    inject(
        &mut client,
        TickEvents {
            tick,
            combat: vec![],
            weapon: vec![(me, fire()), (PlayerId(0), fire())],
            mech: vec![],
        },
    );
    let mut partner = false;
    for _ in 0..60 {
        round(&mut host, &mut client);
        assert_eq!(own_fires(&mut client), 0, "a relayed shot of our own");
        partner |= client
            .world()
            .resource::<FxQueue>()
            .weapon
            .contains(&(PlayerId(0), fire()));
    }
    assert!(partner, "the partner's shot was released");
}

// --- Clients follow the host's screens ---------------------------------------------------

/// What the real game runs around the sim: level stats and episodes, co-op respawns, the HUD
/// and the menus.
fn game_plugins(app: &mut App) {
    app.add_plugins((
        EpisodePlugin,
        CoopPlugin,
        HudPlugin,
        MenuPlugin { enabled: true },
    ));
}

/// [`host_app`] with the [`game_plugins`], hosting `level`.
fn game_host(level: &str) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    insert_level(&mut app, load_map(level), Difficulty::Normal);
    app.insert_resource(LevelFile(level.into()));
    insert_defs(&mut app, Defs::builtin());
    sim_plugins(&mut app);
    game_plugins(&mut app);
    app.update();
    start_hosting(
        app.world_mut(),
        "127.0.0.1:0".parse().unwrap(),
        "Host".into(),
    )
    .expect("host");
    app
}

/// A joined client with the [`game_plugins`].
fn game_client(host: &mut App) -> App {
    let mut client = client_with(game_plugins);
    start_joining(client.world_mut(), host_addr(host), "Tester".into()).expect("client");
    wait_joined(host, &mut client);
    client
}

fn episode(levels: &[&str]) -> Episode {
    Episode::new(EpisodeDef {
        name: "Test Run".into(),
        levels: levels.iter().map(|l| l.to_string()).collect(),
    })
}

fn state(app: &App) -> PlayState {
    *app.world().resource::<PlayState>()
}

fn screen(app: &App) -> Option<Screen> {
    app.world().resource::<MenuScreen>().0
}

fn texts(app: &mut App) -> Vec<String> {
    app.world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|t| t.0.clone())
        .collect()
}

fn has_text(app: &mut App, s: &str) -> bool {
    texts(app).iter().any(|t| t == s)
}

fn kill(host: &mut App, id: PlayerId) {
    let mut q = host.world_mut().query::<(&Player, &mut PlayerVitals)>();
    let (_, mut v) = q
        .iter_mut(host.world_mut())
        .find(|(p, _)| p.id == id)
        .unwrap();
    v.0.damage(10_000);
}

/// The local player's Fire press (its next tick reads it).
fn press_fire(app: &mut App) {
    let mut q = app
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    q.single_mut(app.world_mut()).unwrap().fire_pressed = true;
}

/// Pumps until both play states are `want` (at most 300 rounds).
fn pump_until_both(host: &mut App, client: &mut App, want: PlayState) {
    pump_until(host, client, &format!("both {want:?}"), |h, c| {
        state(h) == want && state(c) == want
    });
}

/// The client's player walks up to the exit switch (the host moves it there), faces it and
/// presses Use; the host's level completes.
fn client_uses_the_exit(host: &mut App, client: &mut App) {
    let map = host.world().resource::<CurrentMap>().0.clone();
    let switch = map
        .switches
        .iter()
        .find(|s| s.action == SwitchAction::Exit)
        .expect("an exit switch");
    let wall = &map.walls[switch.wall];
    let n = wall.inward_normal();
    let spot = (wall.a + wall.b) / 2.0 + n * 0.8;
    {
        let mut q = host.world_mut().query::<(&Player, &mut PlayerBody)>();
        let (_, mut body) = q
            .iter_mut(host.world_mut())
            .find(|(p, _)| p.id == PlayerId(1))
            .unwrap();
        let at = Body::spawn(&map, spot, body.0.radius, body.0.height).expect("in front of it");
        body.0.pos = at.pos;
        body.0.sector = at.sector;
        body.0.vel = Vec3::ZERO;
    }
    let mut q = client
        .world_mut()
        .query_filtered::<(&mut Look, &mut PendingInput), With<LocalPlayer>>();
    let (mut look, mut input) = q.single_mut(client.world_mut()).unwrap();
    look.angle = (-n).to_angle();
    input.use_pressed = true;
}

#[test]
fn client_sees_the_death_screen_and_the_restart() {
    let mut host = game_host("test_yard.ron");
    let mut client = game_client(&mut host);
    kill(&mut host, PlayerId(0));
    kill(&mut host, PlayerId(1));
    let mut rounds = 0;
    while state(&client) != PlayState::Dead {
        rounds += 1;
        assert!(rounds <= 30, "the client never saw the death screen");
        round(&mut host, &mut client);
    }
    assert_eq!(state(&host), PlayState::Dead);
    round(&mut host, &mut client);
    assert!(
        has_text(&mut client, "Everyone's down \u{2014} waiting for the host"),
        "{:?}",
        texts(&mut client)
    );
    assert!(has_text(
        &mut host,
        "You died \u{2014} press Use or Fire to restart"
    ));
    let runs = client.world().resource::<LevelRuns>().0;
    pump(&mut host, &mut client, 80);
    assert_eq!(state(&client), PlayState::Dead, "only the host restarts");
    press_fire(&mut host);
    pump_until_both(&mut host, &mut client, PlayState::Playing);
    pump(&mut host, &mut client, 20);
    assert_eq!(client.world().resource::<LevelRuns>().0, runs + 1);
    assert_eq!(state(&client), PlayState::Playing);
}

/// Host and client on `combat_arena`, level 0 of a two-level episode, both on the stats screen
/// after the client used the exit. The host's tallies carry kills by both players.
fn completed_by_the_client(client_episode: bool) -> (App, App) {
    let mut host = game_host("combat_arena.ron");
    host.insert_resource(episode(&["combat_arena.ron", "test_yard.ron"]));
    let mut client = game_client(&mut host);
    if client_episode {
        // The real game always has the title screen's episode, also when joined.
        client.insert_resource(episode(&["test_yard.ron"]));
    }
    {
        let mut stats = host.world_mut().resource_mut::<Stats>();
        stats.0.kills = 3;
        stats.0.kills_by = [1, 2, 0, 0];
    }
    client_uses_the_exit(&mut host, &mut client);
    pump_until_both(&mut host, &mut client, PlayState::Complete);
    round(&mut host, &mut client);
    (host, client)
}

#[test]
fn exit_by_a_client_completes_for_both() {
    let (mut host, mut client) = completed_by_the_client(false);
    let host_stats = host.world().resource::<Stats>().0;
    assert_eq!(host_stats.kills_by, [1, 2, 0, 0]);
    assert_eq!(client.world().resource::<Stats>().0, host_stats);
    assert!(client.world().resource::<HostRun>().episode);
    assert_eq!(screen(&client), Some(Screen::Stats));
    assert_eq!(screen(&host), Some(Screen::Stats));
    for app in [&mut host, &mut client] {
        assert!(has_text(app, "Host  1"), "{:?}", texts(app));
        assert!(has_text(app, "Tester  2"), "{:?}", texts(app));
    }
    assert!(has_text(&mut client, "Waiting for the host"));
    assert!(!has_text(&mut client, "Continue"));
    assert!(has_text(&mut host, "Continue"));
    assert!(!has_text(&mut host, "Waiting for the host"));
}

#[test]
fn only_the_host_continues() {
    let (mut host, mut client) = completed_by_the_client(true);
    let runs = client.world().resource::<LevelRuns>().0;
    // Two seconds of the client pressing everything it can.
    for _ in 0..120 {
        {
            let mut q = client
                .world_mut()
                .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
            let mut input = q.single_mut(client.world_mut()).unwrap();
            input.fire_pressed = true;
            input.use_pressed = true;
        }
        client.world_mut().resource_mut::<MenuInput>().activate = Some(MenuAction::Continue);
        round(&mut host, &mut client);
    }
    assert_eq!(state(&host), PlayState::Complete);
    assert_eq!(state(&client), PlayState::Complete);
    assert_eq!(client.world().resource::<LevelRuns>().0, runs);
    assert_eq!(host.world().resource::<LevelFile>().0, "combat_arena.ron");
    assert_eq!(host.world().resource::<Episode>().index, 0);
    press_fire(&mut host);
    pump_until(&mut host, &mut client, "level 1 on both", |h, c| {
        state(h) == PlayState::Playing
            && state(c) == PlayState::Playing
            && c.world().resource::<LevelFile>().0 == "test_yard.ron"
    });
    assert_eq!(host.world().resource::<LevelFile>().0, "test_yard.ron");
    assert_eq!(client.world().resource::<LevelRuns>().0, runs + 1);
}

#[test]
fn joiner_during_death_screen_sees_it() {
    let mut host = game_host("test_yard.ron");
    kill(&mut host, PlayerId(0));
    pump_host(&mut host, 2);
    assert_eq!(state(&host), PlayState::Dead);
    let mut client = client_with(game_plugins);
    start_joining(client.world_mut(), host_addr(&host), "Tester".into()).expect("client");
    pump_until(&mut host, &mut client, "the death screen", |_, c| {
        state(c) == PlayState::Dead
    });
    assert_eq!(state(&host), PlayState::Dead);
}

#[test]
fn host_restart_keeps_the_client_pause_menu() {
    let mut host = game_host("test_yard.ron");
    let mut client = game_client(&mut host);
    *client.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    pump(&mut host, &mut client, 2);
    assert_eq!(screen(&client), Some(Screen::Pause));
    let runs = client.world().resource::<LevelRuns>().0;
    restart_level(host.world_mut());
    pump_until(&mut host, &mut client, "the reload", |_, c| {
        c.world().resource::<LevelRuns>().0 > runs
    });
    pump(&mut host, &mut client, 10);
    assert_eq!(state(&client), PlayState::Paused);
    assert_eq!(
        *client.world().resource::<PausedFrom>(),
        PausedFrom(PlayState::Playing)
    );
    assert_eq!(screen(&client), Some(Screen::Pause));
}

#[test]
fn episode_end_reaches_the_client() {
    let mut host = game_host("test_yard.ron");
    host.insert_resource(episode(&["test_yard.ron"]));
    let mut client = game_client(&mut host);
    host.world_mut().resource_mut::<Stats>().0.kills_by = [4, 1, 0, 0];
    *host.world_mut().resource_mut::<PlayState>() = PlayState::Complete;
    pump_until_both(&mut host, &mut client, PlayState::Complete);
    host.world_mut().resource_mut::<AdvanceRequested>().0 = true;
    pump_until_both(&mut host, &mut client, PlayState::EpisodeEnd);
    round(&mut host, &mut client);
    let totals = host.world().resource::<Episode>().totals;
    assert_eq!(totals.kills_by, [4, 1, 0, 0]);
    let run = client.world().resource::<HostRun>();
    assert_eq!(run.name, "Test Run");
    assert_eq!(run.totals, totals);
    assert_eq!(screen(&client), Some(Screen::EpisodeEnd));
    assert!(has_text(&mut client, "Test Run"));
    assert!(has_text(&mut client, "Leave game"));
    assert!(!has_text(&mut client, "Main menu"));
    *host.world_mut().resource_mut::<PlayState>() = PlayState::Menu;
    pump_until(&mut host, &mut client, "the lobby", |_, c| {
        state(c) == PlayState::Menu
    });
    round(&mut host, &mut client);
    assert_eq!(screen(&client), Some(Screen::Lobby));
}
