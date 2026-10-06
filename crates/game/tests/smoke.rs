//! Headless smoke run over every shipped level: scripted walk, turn, fire and weapon cycling,
//! then check the player and every living actor are still inside the map.

use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::map::Map;
use rr_game::combat::{CombatSimPlugin, LevelCombat, PlayerVitals, insert_defs};
use rr_game::flow::FlowPlugin;
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::paths::assets_dir;
use rr_game::player::{Look, PendingInput, PlayerBody, PlayerSimPlugin};

const TICKS: usize = 600;

fn levels() -> Vec<(String, Map)> {
    let dir = assets_dir().join("levels");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let src = std::fs::read_to_string(&p).unwrap();
            let map = Map::from_ron(&src).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, map)
        })
        .collect()
}

fn check(app: &mut App, map: &Map, name: &str, tick: usize) {
    let world = app.world_mut();
    let b = world.query::<&PlayerBody>().single(world).unwrap().0;
    assert!(
        map.find_sector(Vec2::new(b.pos.x, b.pos.y), None).is_some(),
        "{name} tick {tick}: player left the map at {:?}",
        b.pos
    );
    let hp = world
        .query::<&PlayerVitals>()
        .single(world)
        .unwrap()
        .0
        .health
        .hp;
    assert!((0..=100).contains(&hp), "{name} tick {tick}: health {hp}");
    for (i, a) in world
        .resource::<LevelCombat>()
        .0
        .actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.alive())
    {
        assert!(
            map.find_sector(Vec2::new(a.body.pos.x, a.body.pos.y), None)
                .is_some(),
            "{name} tick {tick}: actor {i} left the map at {:?}",
            a.body.pos
        );
    }
}

#[test]
fn every_shipped_level_survives_scripted_play() {
    let levels = levels();
    assert!(levels.len() >= 4, "found only {} levels", levels.len());
    assert!(
        levels.iter().any(|(name, _)| name == "arsenal_depot.ron"),
        "the M4a showcase level is in the scripted play"
    );
    for (name, map) in levels {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        insert_level(&mut app, map.clone(), Difficulty::Normal);
        insert_defs(&mut app, Defs::builtin());
        app.add_plugins((
            FlowPlugin,
            PlayerSimPlugin,
            MechanicsSimPlugin,
            CombatSimPlugin,
        ));
        app.update();
        for t in 0..TICKS {
            {
                let world = app.world_mut();
                let mut q = world.query::<(&mut PendingInput, &mut Look)>();
                let (mut input, mut look) = q.single_mut(world).unwrap();
                input.forward = 1.0;
                input.fire = true;
                input.fire_pressed = true;
                if t % 100 == 99 {
                    input.cycle = 1;
                }
                look.angle += 0.01 + 0.02 * ((t / 60) % 3) as f32;
            }
            app.world_mut().run_schedule(FixedUpdate);
            if t % 25 == 0 || t == TICKS - 1 {
                check(&mut app, &map, &name, t);
            }
        }
        println!("smoke ok: {name}");
    }
}
