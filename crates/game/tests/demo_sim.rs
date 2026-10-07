//! A demo script drives the player headless: it walks forward, then turns, then the app exits.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
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
    insert_level(&mut app, hall(), Difficulty::Normal);
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

/// Runs the shipped `engine_lab` showcase demo headless at the recording rate (two fixed ticks
/// per frame, as `--record` does) and checks it ends on the level-complete screen: the boss's
/// `on_death` exit fired. With `DEMO_TRACE=1` it prints the run, for tuning the script.
#[test]
fn engine_lab_demo_ends_on_level_complete() {
    use rr_game::combat::{LevelCombat, PlayerVitals};
    use rr_game::flow::PlayState;
    use rr_game::mechanics::{HudMessage, LevelMechanics};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let script = rr_game::demo::load_script(&dir.join("demo/engine_lab.ron"));
    let map = rr_game::level::load_map("engine_lab.ron");
    let trace = std::env::var_os("DEMO_TRACE").is_some();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::from_secs(1) / rr_game::demo::RECORD_FPS,
        ));
    insert_level(&mut app, map.clone(), Difficulty::Normal);
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

    let mut complete_at = None;
    let mut last = PlayState::Playing;
    for frame in 0..30 * 120 {
        app.update();
        let world = app.world_mut();
        let state = *world.resource::<PlayState>();
        if trace && frame % 6 == 0 {
            let (body, look, vitals) = world
                .query::<(&PlayerBody, &Look, &PlayerVitals)>()
                .single(world)
                .unwrap();
            let p = body.0.pos;
            let sector = map.find_sector(p.truncate(), None);
            let actors: Vec<String> = world
                .resource::<LevelCombat>()
                .0
                .actors
                .iter()
                .map(|a| {
                    format!(
                        "{:?}@({:.1},{:.1}) {} {:?}",
                        a.kind, a.body.pos.x, a.body.pos.y, a.health.hp, a.state
                    )
                })
                .collect();
            let msg = world
                .get_resource::<HudMessage>()
                .filter(|m| m.remaining > 0.0)
                .map(|m| m.text.clone())
                .unwrap_or_default();
            let props: Vec<_> = world
                .resource::<LevelMechanics>()
                .0
                .props
                .iter()
                .map(|p| (p.stock, p.cooldown))
                .collect();
            eprintln!(
                "t={:5.1} ({:5.2},{:5.2},{:5.2}) hd {:4.0} s{:?} hp{} {state:?} props{props:?} {actors:?} {msg}",
                frame as f32 / 30.0,
                p.x,
                p.y,
                p.z,
                look.angle.to_degrees(),
                sector,
                vitals.0.health.hp,
            );
        }
        last = state;
        if state == PlayState::Complete && complete_at.is_none() {
            complete_at = Some(frame);
        }
        if app.should_exit().is_some() {
            break;
        }
    }
    assert!(
        complete_at.is_some(),
        "the demo kills the boss and completes the level"
    );
    assert_eq!(last, PlayState::Complete, "and stays on the stats screen");
}
