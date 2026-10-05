//! Decals: ring cap, removal when a sector is rebuilt, and clearing on restart.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::fixtures::door_rooms;
use rr_game::combat::{CombatSimPlugin, FxQueue, insert_defs};
use rr_game::decals::{Decal, DecalRing, DecalsPlugin, RING_CAP};
use rr_game::flow::{FlowPlugin, LevelEntity, restart_level};
use rr_game::level::LevelRenderPlugin;
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
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::Impact {
            point: Vec3::new(1.0, 1.0, 1.0),
            normal: Vec3::X,
            sector,
        });
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
