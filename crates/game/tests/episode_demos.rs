//! Each episode level's demo route plays headless at the recording rate and ends on the
//! level-complete screen. `DEMO_TRACE=1` prints the run (tuning aid).
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::demo::DemoPlugin;
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{PlayerBody, PlayerSimPlugin};
use std::time::Duration;

fn route_completes(level: &str, max_secs: u32) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let stem = level.trim_end_matches(".ron");
    let script = rr_game::demo::load_script(&dir.join(format!("demo/{stem}.ron")));
    let map = rr_game::level::load_map(level);
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
    for frame in 0..rr_game::demo::RECORD_FPS * max_secs {
        app.update();
        let world = app.world_mut();
        if trace && frame % 15 == 0 {
            let p = world.query::<&PlayerBody>().single(world).unwrap().0.pos;
            eprintln!(
                "t={:6.1} ({:6.2},{:6.2},{:5.2}) s{:?}",
                frame as f32 / 30.0,
                p.x,
                p.y,
                p.z,
                map.find_sector(p.truncate(), None)
            );
        }
        if *world.resource::<PlayState>() == PlayState::Complete {
            return;
        }
        if app.should_exit().is_some() {
            break;
        }
    }
    panic!("{level}: the demo route never reached the level-complete screen");
}

#[test]
fn hollywood_meltdown_route_completes() {
    route_completes("hollywood_meltdown.ron", 600);
}

#[test]
fn neon_nights_route_completes() {
    route_completes("neon_nights.ron", 700);
}
