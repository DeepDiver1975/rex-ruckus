//! Viewmodel headless: one model per weapon under the persistent camera, surviving restarts.

use bevy::prelude::*;
use rr_core::defs::{Defs, WeaponId};
use rr_core::fixtures::combat_room;
use rr_core::weapons::WeaponEvent;
use rr_game::combat::{CombatSimPlugin, FxQueue, insert_defs};
use rr_game::flow::{FlowPlugin, LevelEntity, PlayState, restart_level};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{PlayerCamera, PlayerSimPlugin, spawn_camera};
use rr_game::viewmodel::{KickLeg, MuzzleFlash, ViewModel, ViewModelPlugin, ViewRig};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>();
    insert_level(&mut app, combat_room());
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        ViewModelPlugin,
    ))
    .add_systems(Startup, spawn_camera);
    app.update();
    app
}

fn count<C: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), With<C>>();
    q.iter(app.world()).count()
}

fn visible_flashes(app: &mut App) -> usize {
    let mut q = app
        .world_mut()
        .query_filtered::<&Visibility, With<MuzzleFlash>>();
    q.iter(app.world())
        .filter(|v| **v != Visibility::Hidden)
        .count()
}

fn rig_visibility(app: &mut App) -> Visibility {
    let mut q = app
        .world_mut()
        .query_filtered::<&Visibility, With<ViewRig>>();
    *q.single(app.world()).unwrap()
}

#[test]
fn one_model_per_weapon_under_the_camera_survives_restart() {
    let mut app = app();
    let check = |app: &mut App| {
        let mut q = app.world_mut().query::<&ViewModel>();
        let mut ws: Vec<_> = q.iter(app.world()).map(|m| m.0).collect();
        ws.sort_by_key(|w| w.index());
        assert_eq!(ws, WeaponId::ALL.to_vec());
        assert_eq!(count::<KickLeg>(app), 1);
        assert_eq!(count::<PlayerCamera>(app), 1);
        let mut q = app.world_mut().query_filtered::<&ChildOf, With<ViewRig>>();
        let cam = q.single(app.world()).unwrap().parent();
        assert!(app.world().get::<PlayerCamera>(cam).is_some());
        let mut q = app
            .world_mut()
            .query_filtered::<(), (With<ViewModel>, With<LevelEntity>)>();
        assert_eq!(q.iter(app.world()).count(), 0);
    };
    check(&mut app);
    app.world_mut().run_system_cached(restart_level).unwrap();
    app.update();
    check(&mut app);
}

#[test]
fn flash_follows_fire_and_view_hides_when_dead() {
    let mut app = app();
    app.update();
    assert_eq!(visible_flashes(&mut app), 0);
    assert_eq!(rig_visibility(&mut app), Visibility::Visible);
    app.world_mut()
        .resource_mut::<FxQueue>()
        .weapon
        .push(WeaponEvent::Fire {
            weapon: WeaponId::Pistol,
            dirs: vec![Vec3::X],
        });
    app.update();
    assert!(visible_flashes(&mut app) > 0, "flash after Fire");
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Dead;
    app.update();
    assert_eq!(rig_visibility(&mut app), Visibility::Hidden);
}
