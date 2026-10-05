//! Decals: ring cap, removal when a sector is rebuilt, and clearing on restart.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::fixtures::door_rooms;
use rr_game::combat::{CombatSimPlugin, FxQueue, insert_defs};
use rr_game::decals::{Decal, DecalRing, DecalsPlugin, RING_CAP};
use rr_game::flow::{FlowPlugin, LevelEntity, restart_level};
use rr_game::level::{CurrentMap, LevelRenderPlugin};
use rr_game::mechanics::{DirtySectors, MechanicsSimPlugin, insert_level};
use rr_game::player::PlayerSimPlugin;
use std::time::Duration;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    insert_level(&mut app, door_rooms("(kind: Door)", ""));
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        LevelRenderPlugin,
        DecalsPlugin,
    ));
    app.update();
    app
}

fn impact(app: &mut App, sector: usize) {
    impact_on(app, sector, None);
}

fn impact_on(app: &mut App, sector: usize, wall: Option<usize>) {
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::Impact {
            point: Vec3::new(1.0, 1.0, 1.0),
            normal: Vec3::X,
            sector,
            wall,
        });
}

/// A wall of `sector` whose far side is `next` (`None`: a solid wall).
fn wall_of(app: &App, sector: usize, next: Option<usize>) -> usize {
    let map = &app.world().resource::<CurrentMap>().0;
    (0..map.walls.len())
        .find(|&w| map.walls[w].sector == sector && map.walls[w].next_sector == next)
        .expect("fixture has such a wall")
}

fn decals(app: &mut App) -> Vec<usize> {
    let mut q = app.world_mut().query::<&Decal>();
    q.iter(app.world()).map(|d| d.sector).collect()
}

#[test]
fn decal_ring_caps_at_128() {
    let mut app = app();
    for _ in 0..RING_CAP + 10 {
        impact(&mut app, 0);
    }
    app.update();
    assert_eq!(decals(&mut app).len(), RING_CAP);
    assert_eq!(app.world().resource::<DecalRing>().len(), RING_CAP);
}

#[test]
fn decals_cleared_on_sector_rebuild() {
    let mut app = app();
    impact(&mut app, 0);
    impact(&mut app, 1);
    impact(&mut app, 1);
    app.update();
    assert_eq!(decals(&mut app).len(), 3);
    // Only the dirty sector's own decals go: room 0 next door is untouched.
    app.world_mut().resource_mut::<DirtySectors>().0.insert(1);
    app.update();
    assert_eq!(decals(&mut app), vec![0]);
    assert_eq!(app.world().resource::<DecalRing>().len(), 1);
}

#[test]
fn decals_cleared_on_restart() {
    let mut app = app();
    impact(&mut app, 0);
    app.update();
    assert_eq!(decals(&mut app).len(), 1);
    let mut q = app.world_mut().query_filtered::<(), With<Decal>>();
    assert_eq!(q.iter(app.world()).count(), 1);
    let mut q = app
        .world_mut()
        .query_filtered::<(), (With<Decal>, With<LevelEntity>)>();
    assert_eq!(q.iter(app.world()).count(), 1, "decals are level entities");
    restart_level(app.world_mut());
    assert!(decals(&mut app).is_empty());
    assert_eq!(app.world().resource::<DecalRing>().len(), 0);
}

#[test]
fn door_step_face_decal_purged_but_solid_wall_decal_kept() {
    let mut app = app();
    // Door is sector 1 between rooms 0 and 2. One hole on room 0's portal wall into the door (the
    // door's face as seen from room 0), one on a solid wall of room 0, one on room 0's floor.
    let face = wall_of(&app, 0, Some(1));
    let solid = wall_of(&app, 0, None);
    impact_on(&mut app, 0, Some(face));
    impact_on(&mut app, 0, Some(solid));
    impact_on(&mut app, 0, None);
    app.update();
    assert_eq!(decals(&mut app).len(), 3);
    // The moving door dirties only its own sector.
    app.world_mut().resource_mut::<DirtySectors>().0.insert(1);
    app.update();
    let mut q = app.world_mut().query::<&Decal>();
    let mut left: Vec<_> = q.iter(app.world()).map(|d| d.wall).collect();
    left.sort();
    assert_eq!(
        left,
        vec![None, Some(solid)],
        "only the door-face hole is purged"
    );
    assert_eq!(app.world().resource::<DecalRing>().len(), 2);
}
