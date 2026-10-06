use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::door_rooms;
use rr_core::map::{Key, Map};
use rr_core::mechanics::{MechEvent, Motion};
use rr_game::combat::{CombatSimPlugin, FxQueue, clear_fx, insert_defs};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::level::CurrentMap;
use rr_game::mechanics::{
    DirtySectors, HudMessage, LevelMechanics, MechanicsSimPlugin, insert_level,
};
use rr_game::player::{Inventory, PendingInput, PlayerBody, PlayerSimPlugin};

fn app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map, Difficulty::Normal);
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

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn input(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app.world_mut().query::<&mut PendingInput>();
    q.single_mut(app.world_mut()).unwrap()
}

fn body(app: &mut App) -> rr_core::collide::Body {
    let mut q = app.world_mut().query::<&PlayerBody>();
    q.single(app.world()).unwrap().0
}

/// Walk east until the closed door stops us (start (2,2) faces east).
fn walk_to_door(app: &mut App) {
    input(app).forward = 1.0;
    ticks(app, 40);
    input(app).forward = 0.0;
    ticks(app, 30);
}

#[test]
fn level_starts_with_doors_closed() {
    let app = app(door_rooms("(kind: Door)", ""));
    assert_eq!(
        app.world().resource::<CurrentMap>().0.sectors[1].ceil_z,
        0.0
    );
}

#[test]
fn pressing_use_opens_the_door_and_player_walks_through() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    walk_to_door(&mut app);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 80);
    assert_eq!(
        app.world().resource::<CurrentMap>().0.sectors[1].ceil_z,
        3.0
    );
    assert!(app.world().resource::<DirtySectors>().0.contains(&1));
    input(&mut app).forward = 1.0;
    ticks(&mut app, 60);
    assert_eq!(body(&mut app).sector, 2);
}

#[test]
fn use_press_is_consumed_once() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    walk_to_door(&mut app);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert!(!input(&mut app).use_pressed, "latch cleared by the tick");
    ticks(&mut app, 5);
    assert_eq!(
        app.world().resource::<LevelMechanics>().0.movers[0].motion,
        Motion::ToEnd,
        "not toggled back"
    );
}

#[test]
fn locked_door_needs_the_keycard_lying_in_the_room() {
    let mut app = app(door_rooms(
        "(kind: Door, lock: Some(Red))",
        "items: [(kind: Key(Red), pos: (3.0, 1.5))],",
    ));
    // Walking east passes within reach of the key at (3,1.5).
    walk_to_door(&mut app);
    {
        let mut q = app.world_mut().query::<&Inventory>();
        assert!(q.single(app.world()).unwrap().keys.contains(Key::Red));
    }
    assert!(app.world().resource::<HudMessage>().text.contains("red"));
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<LevelMechanics>().0.movers[0].motion,
        Motion::ToEnd
    );
}

#[test]
fn locked_door_without_key_shows_message() {
    let mut app = app(door_rooms("(kind: Door, lock: Some(Red))", ""));
    walk_to_door(&mut app);
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(
        app.world().resource::<LevelMechanics>().0.movers[0].motion,
        Motion::AtStart
    );
    assert_eq!(
        app.world().resource::<HudMessage>().text,
        "You need the red keycard"
    );
}

#[test]
fn exit_switch_completes_the_level() {
    let mut app = app(door_rooms(
        "(kind: Door)",
        "switches: [(wall: (7, 0), action: Exit)],",
    ));
    {
        let mut q = app
            .world_mut()
            .query::<(&mut PlayerBody, &mut rr_game::player::Look)>();
        let (mut b, mut look) = q.single_mut(app.world_mut()).unwrap();
        b.0.pos.x = 1.0;
        look.angle = std::f32::consts::PI;
    }
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Complete);
}

#[test]
fn using_a_door_queues_mover_started_until_clear_fx() {
    let mut app = app(door_rooms("(kind: Door)", ""));
    walk_to_door(&mut app);
    assert!(app.world().resource::<FxQueue>().mech.is_empty());
    input(&mut app).use_pressed = true;
    ticks(&mut app, 1);
    let mech = &app.world().resource::<FxQueue>().mech;
    assert!(
        mech.iter()
            .any(|e| matches!(e, MechEvent::MoverStarted { .. })),
        "{mech:?}"
    );
    app.world_mut().run_system_once(clear_fx).unwrap();
    assert!(app.world().resource::<FxQueue>().mech.is_empty());
}

#[test]
fn picking_up_an_item_queues_item_taken() {
    let mut app = app(door_rooms(
        "(kind: Door)",
        "items: [(kind: Key(Red), pos: (2.3, 2.0))],",
    ));
    ticks(&mut app, 1);
    let mech = &app.world().resource::<FxQueue>().mech;
    assert!(
        mech.iter()
            .any(|e| matches!(e, MechEvent::ItemTaken { .. })),
        "{mech:?}"
    );
}
