use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::fixtures::{combat_room, door_rooms};
use rr_core::map::{ActorKind, ActorSpawn, Item, ItemKind, Key, Map, Switch, SwitchAction};
use rr_core::mechanics::Motion;
use rr_core::weapons::Arsenal;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, LevelCombat, PlayerArsenal, PlayerHealth, insert_defs,
};
use rr_game::flow::{FlowPlugin, LevelEntity, PlayState, StateAge, restart_requested};
use rr_game::level::{CurrentMap, LevelRenderPlugin};
use rr_game::mechanics::{HudMessage, LevelMechanics, MechanicsSimPlugin, insert_level};
use rr_game::player::{
    Inventory, Look, PendingInput, PlayerBody, PlayerCamera, PlayerSimPlugin, spawn_camera,
};
use rr_game::props::PropsPlugin;

fn sim_app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
    ));
    app.update();
    app
}

/// The headless sim plus level meshes, lights, props and the camera (no window, no GPU).
fn full_app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>();
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        LevelRenderPlugin,
        PropsPlugin,
    ))
    .add_systems(Startup, spawn_camera);
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn input(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app.world_mut().query::<&mut PendingInput>();
    q.single_mut(app.world_mut()).unwrap()
}

fn health(app: &mut App) -> Mut<'_, PlayerHealth> {
    let mut q = app.world_mut().query::<&mut PlayerHealth>();
    q.single_mut(app.world_mut()).unwrap()
}

fn body(app: &mut App) -> rr_core::collide::Body {
    let mut q = app.world_mut().query::<&PlayerBody>();
    q.single(app.world()).unwrap().0
}

fn state(app: &App) -> PlayState {
    *app.world().resource::<PlayState>()
}

fn door_z(app: &App) -> f32 {
    app.world().resource::<LevelMechanics>().0.movers[0].z
}

fn kill_player(app: &mut App) {
    health(app).0.damage(1000);
    ticks(app, 1);
    assert_eq!(state(app), PlayState::Dead);
}

/// Waits out the death screen's grace period, then presses use.
fn restart(app: &mut App) {
    ticks(app, 70);
    input(app).use_pressed = true;
    ticks(app, 1);
    assert_eq!(state(app), PlayState::Playing, "restarted");
}

fn red_key(x: f32, y: f32) -> Item {
    Item {
        kind: ItemKind::Key(Key::Red),
        pos: Vec2::new(x, y),
    }
}

/// combat_room plus one awake grunt in the main room.
fn arena() -> Map {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(7.0, 1.5),
        angle: std::f32::consts::PI,
        asleep: false,
    });
    map
}

#[test]
fn restart_waits_and_needs_a_press() {
    use PlayState::*;
    assert!(
        !restart_requested(Playing, 5.0, true, true),
        "never while playing"
    );
    assert!(!restart_requested(Dead, 0.5, true, true), "not before 1 s");
    assert!(!restart_requested(Dead, 3.0, false, false), "needs a press");
    assert!(restart_requested(Dead, 1.0, true, false));
    assert!(restart_requested(Complete, 2.0, false, true));
}

#[test]
fn early_press_is_consumed_not_deferred() {
    let mut app = sim_app(combat_room());
    kill_player(&mut app);
    ticks(&mut app, 10);
    input(&mut app).use_pressed = true;
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    assert!(!input(&mut app).use_pressed && !input(&mut app).fire_pressed);
    ticks(&mut app, 120);
    assert_eq!(
        state(&app),
        PlayState::Dead,
        "the early press did not linger"
    );
    assert!(app.world().resource::<StateAge>().0 >= 1.0);
}

#[test]
fn death_freezes_play() {
    let mut app = sim_app(arena());
    // Let the grunt get past its reaction time, then start the door.
    ticks(&mut app, 60);
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    let grunt_pos = |app: &App| app.world().resource::<LevelCombat>().0.actors[0].body.pos;
    let (z0, grunt0) = (door_z(&app), grunt_pos(&app));
    ticks(&mut app, 5);
    assert!(door_z(&app) > z0, "door is opening");
    assert_ne!(grunt_pos(&app), grunt0, "the grunt is moving");

    kill_player(&mut app);
    let (z, grunt, me) = (door_z(&app), grunt_pos(&app), body(&mut app).pos);
    assert!(z < 3.0, "door still mid-motion");
    input(&mut app).forward = 1.0;
    ticks(&mut app, 60);
    assert_eq!(door_z(&app), z, "door frozen mid-motion");
    assert_eq!(grunt_pos(&app), grunt, "grunt frozen");
    assert_eq!(body(&mut app).pos, me, "player frozen");
    assert_eq!(state(&app), PlayState::Dead);
}

#[test]
fn level_complete_freezes_play() {
    let mut app = sim_app(door_rooms(
        "(kind: Door)",
        "switches: [(wall: (7, 0), action: Exit)],",
    ));
    {
        let mut q = app.world_mut().query::<(&mut PlayerBody, &mut Look)>();
        let (mut b, mut look) = q.single_mut(app.world_mut()).unwrap();
        b.0.pos.x = 1.0;
        look.angle = std::f32::consts::PI;
    }
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    ticks(&mut app, 5);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(state(&app), PlayState::Complete);
    assert_eq!(app.world().resource::<HudMessage>().text, "Level complete!");
    let (z, me) = (door_z(&app), body(&mut app).pos);
    input(&mut app).forward = 1.0;
    ticks(&mut app, 60);
    assert_eq!(door_z(&app), z, "door frozen mid-motion");
    assert_eq!(body(&mut app).pos, me, "player frozen");
    assert_eq!(state(&app), PlayState::Complete);
}

#[test]
fn restart_resets_world_state() {
    let mut map = arena();
    map.items.push(red_key(1.5, 1.5));
    let mut app = sim_app(map);
    // Pick up the key, open the door, shoot, and hurt the grunt.
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 120);
    assert_eq!(
        app.world().resource::<CurrentMap>().0.sectors[3].ceil_z,
        3.0
    );
    assert!(app.world().resource::<LevelMechanics>().0.taken[0]);
    {
        let mut q = app.world_mut().query::<&Inventory>();
        assert!(q.single(app.world()).unwrap().keys.contains(Key::Red));
    }
    {
        let mut combat = app.world_mut().resource_mut::<LevelCombat>();
        combat.0.actors[0].body.pos.x -= 1.0;
        combat.0.actors[0].health.damage(1000);
    }
    let defs = Defs::builtin();
    {
        let mut q = app.world_mut().query::<&PlayerArsenal>();
        assert_ne!(
            q.single(app.world()).unwrap().0,
            Arsenal::new(&defs),
            "ammo spent"
        );
    }

    kill_player(&mut app);
    restart(&mut app);

    let world = app.world();
    assert_eq!(world.resource::<StateAge>().0, 0.0);
    assert_eq!(
        world.resource::<CurrentMap>().0.sectors[3].ceil_z,
        0.0,
        "door closed"
    );
    let mech = &world.resource::<LevelMechanics>().0;
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
    assert!(!mech.taken[0], "key back in the world");
    let actor = &world.resource::<LevelCombat>().0.actors[0];
    assert!(actor.alive());
    assert_eq!(
        actor.body.pos.truncate(),
        Vec2::new(7.0, 1.5),
        "back at its spawn"
    );
    assert!(world.resource::<FxQueue>().combat.is_empty());
    assert!(world.resource::<FxQueue>().weapon.is_empty());
    assert_eq!(world.resource::<HudMessage>().text, "");

    let mut q = app
        .world_mut()
        .query::<(&Inventory, &PlayerArsenal, &PlayerHealth, &PlayerBody)>();
    let (inv, arsenal, hp, b) = q.single(app.world()).unwrap();
    assert!(
        Key::ALL.iter().all(|&k| !inv.keys.contains(k)),
        "inventory empty"
    );
    assert_eq!(arsenal.0, Arsenal::new(&defs), "starting arsenal");
    assert_eq!(hp.0.hp, 100);
    assert_eq!(b.0.pos.truncate(), Vec2::new(1.5, 1.5), "back at the start");
}

fn count_level(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<LevelEntity>>();
    q.iter(app.world()).count()
}

fn count_all(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<Entity>();
    q.iter(app.world()).count()
}

fn cameras(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<PlayerCamera>>();
    q.iter(app.world()).count()
}

#[test]
fn restart_leaves_no_stale_entities() {
    let mut map = arena();
    map.items.push(red_key(3.0, 1.5));
    // The west wall of the main room.
    let wall = map
        .walls
        .iter()
        .position(|w| w.a == Vec2::new(0.0, 8.0) && w.b == Vec2::ZERO)
        .unwrap();
    map.switches.push(Switch {
        wall,
        action: SwitchAction::Exit,
        key: None,
    });
    let mut app = full_app(map);
    let (level, all) = (count_level(&mut app), count_all(&mut app));
    // Player, sector meshes, lights, a key and a switch panel.
    assert!(level > 5, "{level} level entities");
    assert_eq!(cameras(&mut app), 1);
    for _ in 0..2 {
        kill_player(&mut app);
        restart(&mut app);
        app.update();
        assert_eq!(count_level(&mut app), level);
        assert_eq!(count_all(&mut app), all);
        assert_eq!(cameras(&mut app), 1);
    }
}
