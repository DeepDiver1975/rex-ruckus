//! A demo script drives the player headless: it walks forward, then turns, then the app exits.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::defs::Defs;
use rr_core::map::Map;
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::demo::{DemoPlugin, DemoScript};
use rr_game::flow::FlowPlugin;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{Look, PlayerBody, PlayerSimPlugin};
use std::time::Duration;

fn hall() -> Map {
    Map::from_ron(
        r#"(
        name: "hall",
        materials: ["wall"],
        vertices: [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)],
        sectors: [(loops: [[0, 1, 2, 3]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)],
        player_start: (pos: (2.0, 5.0), angle_deg: 0.0),
    )"#,
    )
    .expect("hall is valid")
}

#[test]
fn script_walks_turns_and_exits() {
    let script = DemoScript::from_ron(
        "(segments: [(secs: 0.5, forward: 1.0), (secs: 0.5, turn_deg: 90.0)])",
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            50,
        )));
    insert_level(&mut app, hall());
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        DemoPlugin {
            script,
            record: None,
        },
    ));

    let mut exit = None;
    for _ in 0..60 {
        app.update();
        if let Some(e) = app.should_exit() {
            exit = Some(e);
            break;
        }
    }
    assert_eq!(exit, Some(AppExit::Success), "the demo ends the app");

    let world = app.world_mut();
    let (body, look) = world.query::<(&PlayerBody, &Look)>().single(world).unwrap();
    assert!(body.0.pos.x > 4.0, "walked east: {:?}", body.0.pos);
    assert!(
        (body.0.pos.y - 5.0).abs() < 0.01,
        "walked straight: {:?}",
        body.0.pos
    );
    assert!(
        (look.angle - 90f32.to_radians()).abs() < 1e-3,
        "turned left: {}",
        look.angle
    );
}
