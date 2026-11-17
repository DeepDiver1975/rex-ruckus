//! Co-op in the headless sim: several `Player { id }` entities, one of them the `LocalPlayer`.

use bevy::ecs::system::{RunSystemOnce, SystemId};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use rr_core::actors::Target;
use rr_core::collide::Body;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, engine_room};
use rr_core::map::{ActorKind, ActorSpawn, Map};
use rr_core::players::PlayerId;
use rr_game::combat::{CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerVitals, insert_defs};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::level::CurrentMap;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{
    LocalPlayer, LookSettings, PLAYER_RADIUS, PendingInput, Player, PlayerBody, PlayerSimPlugin,
    PlayerTuning, PrevFeet, read_input, spawn_coop_player,
};

fn app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map, Difficulty::Normal);
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
    ));
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn spawn_coop(app: &mut App, id: PlayerId) -> Entity {
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  map: Res<CurrentMap>,
                  tuning: Res<PlayerTuning>,
                  defs: Res<GameDefs>| {
                spawn_coop_player(&mut commands, &map.0, &tuning.0, &defs.0, id)
            },
        )
        .unwrap()
}

/// The entity of player `id`.
fn player(app: &mut App, id: PlayerId) -> Entity {
    let mut q = app.world_mut().query::<(Entity, &Player)>();
    q.iter(app.world())
        .find(|(_, p)| p.id == id)
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no player {id:?}"))
}

fn body(app: &mut App, id: PlayerId) -> Body {
    let e = player(app, id);
    app.world().get::<PlayerBody>(e).unwrap().0
}

fn health(app: &mut App, id: PlayerId) -> i32 {
    let e = player(app, id);
    app.world().get::<PlayerVitals>(e).unwrap().0.health.hp
}

fn place(app: &mut App, id: PlayerId, x: f32, y: f32) {
    let e = player(app, id);
    let map = app.world().resource::<CurrentMap>().0.clone();
    let mut b = app.world().get::<PlayerBody>(e).unwrap().0;
    let spawned = Body::spawn(&map, Vec2::new(x, y), b.radius, b.height).unwrap();
    b.pos = spawned.pos;
    b.sector = spawned.sector;
    b.vel = Vec3::ZERO;
    app.world_mut().get_mut::<PlayerBody>(e).unwrap().0 = b;
    app.world_mut().get_mut::<PrevFeet>(e).unwrap().0 = b.pos;
}

fn kill(app: &mut App, id: PlayerId) {
    let e = player(app, id);
    app.world_mut()
        .get_mut::<PlayerVitals>(e)
        .unwrap()
        .0
        .damage(1000);
}

#[test]
fn two_players_spawn_apart() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    let (a, b) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    assert!(
        a.pos.truncate().distance(b.pos.truncate()) >= 2.0 * PLAYER_RADIUS,
        "{a:?} {b:?}"
    );
    // The co-op player is no local one and carries the default loadout.
    let e = player(&mut app, PlayerId(1));
    assert!(app.world().get::<LocalPlayer>(e).is_none());
    assert_eq!(health(&mut app, PlayerId(1)), 100);
    let mut q = app
        .world_mut()
        .query_filtered::<&Player, With<LocalPlayer>>();
    let locals: Vec<Player> = q.iter(app.world()).copied().collect();
    assert_eq!(locals, [Player { id: PlayerId(0) }]);
}

/// The sim plus the real input system, with a grabbed cursor and no window.
fn input_rig(map: Map) -> (App, SystemId) {
    let mut app = app(map);
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<rr_game::bindings::Bindings>()
        .insert_resource(LookSettings::default());
    app.world_mut().spawn(CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        ..default()
    });
    let read = app.world_mut().register_system(read_input);
    (app, read)
}

#[test]
fn local_input_moves_only_local_player() {
    let (mut app, read) = input_rig(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    let (a0, b0) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    for _ in 0..10 {
        app.world_mut().run_system(read).unwrap();
        ticks(&mut app, 1);
    }
    let (a1, b1) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    assert!(
        a1.pos.x > a0.pos.x + 0.1,
        "local player moved: {a0:?} -> {a1:?}"
    );
    assert_eq!(b1.pos.truncate(), b0.pos.truncate(), "player 1 stayed");

    // Whoever drives player 1 writes its input directly.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    let e = player(&mut app, PlayerId(1));
    app.world_mut().get_mut::<PendingInput>(e).unwrap().forward = 1.0;
    for _ in 0..10 {
        app.world_mut().run_system(read).unwrap();
        ticks(&mut app, 1);
    }
    let b2 = body(&mut app, PlayerId(1));
    assert!(
        b2.pos.x > b1.pos.x + 0.1,
        "player 1 moved: {b1:?} -> {b2:?}"
    );
}

#[test]
fn enemy_targets_nearer_player() {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(7.0, 7.0),
        angle: std::f32::consts::PI,
        asleep: false,
        skill: Difficulty::Easy,
        on_death: None,
    });
    let mut app = app(map);
    spawn_coop(&mut app, PlayerId(1));
    place(&mut app, PlayerId(1), 5.5, 7.0);
    let mut target = None;
    for _ in 0..30 {
        ticks(&mut app, 1);
        target = Some(app.world().resource::<LevelCombat>().0.actors[0].target);
        if target == Some(Target::Player(PlayerId(1))) {
            break;
        }
    }
    assert_eq!(target, Some(Target::Player(PlayerId(1))));
}

#[test]
fn sim_orders_players_by_id() {
    let mut app = app(engine_room(""));
    // A fresh set of players spawned in reverse order: id 1, then id 0.
    let old = player(&mut app, PlayerId(0));
    app.world_mut().despawn(old);
    spawn_coop(&mut app, PlayerId(1));
    spawn_coop(&mut app, PlayerId(0));
    // Player 1 stands in the slime pit, player 0 in the hall.
    place(&mut app, PlayerId(1), 11.0, 2.0);
    place(&mut app, PlayerId(0), 1.0, 2.0);
    let mut hurt = Vec::new();
    for _ in 0..120 {
        ticks(&mut app, 1);
        let mut fx = app.world_mut().resource_mut::<FxQueue>();
        hurt.extend(fx.combat.drain(..).filter_map(|e| match e {
            CombatEvent::PlayerHurt { player, .. } => Some(player),
            _ => None,
        }));
        if !hurt.is_empty() {
            break;
        }
    }
    assert_eq!(hurt, [PlayerId(1)], "the slime burned player 1");
    assert!(health(&mut app, PlayerId(1)) < 100);
    assert_eq!(health(&mut app, PlayerId(0)), 100);
}

#[test]
fn level_continues_while_a_partner_lives() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    kill(&mut app, PlayerId(0));
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
    // The corpse ignores its input.
    let before = body(&mut app, PlayerId(0));
    let e = player(&mut app, PlayerId(0));
    app.world_mut().get_mut::<PendingInput>(e).unwrap().forward = 1.0;
    ticks(&mut app, 10);
    let after = body(&mut app, PlayerId(0));
    assert_eq!(
        after.pos.truncate(),
        before.pos.truncate(),
        "the dead stay put"
    );

    kill(&mut app, PlayerId(1));
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
}
