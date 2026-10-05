//! Viewmodel headless: one model per weapon under the persistent camera, surviving restarts.

use bevy::prelude::*;
use rr_core::defs::{Defs, WeaponId};
use rr_core::fixtures::combat_room;
use rr_core::weapons::WeaponEvent;
use rr_game::combat::{CombatSimPlugin, FxQueue, insert_defs};
use rr_game::flow::{FlowPlugin, LevelEntity, PlayState, restart_level};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{DEATH_ROLL, PlayerCamera, PlayerSimPlugin, ViewRoll, spawn_camera};
use rr_game::viewmodel::{
    BarrelCluster, Detonator, HeldBomb, KickLeg, MuzzleFlash, Recoil, ViewModel, ViewModelPlugin,
    ViewRig, ViewState,
};

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

#[test]
fn restart_resets_view_state_and_roll() {
    let mut app = app();
    app.init_resource::<ViewRoll>();
    // Warm-up restart: registers the cached `restart_level` system (one entity) up front.
    app.world_mut().run_system_cached(restart_level).unwrap();
    {
        let mut s = app.world_mut().resource_mut::<ViewState>();
        s.last_look = Some(Vec2::new(2.0, 0.4));
        s.recoil = Recoil {
            z: 0.05,
            pitch: 0.1,
        };
        s.flash = 0.04;
        s.kick = 0.2;
        s.sway = Vec2::new(0.01, 0.01);
        s.bob_phase = 1.0;
    }
    app.world_mut().resource_mut::<ViewRoll>().0 = DEATH_ROLL;
    let entities = {
        let mut q = app.world_mut().query::<Entity>();
        q.iter(app.world()).count()
    };
    app.world_mut().run_system_cached(restart_level).unwrap();
    assert_eq!(*app.world().resource::<ViewState>(), ViewState::default());
    assert_eq!(
        app.world().resource::<ViewRoll>().0,
        0.0,
        "roll snaps upright"
    );
    let mut q = app.world_mut().query::<Entity>();
    assert_eq!(q.iter(app.world()).count(), entities, "reset in place");
}

fn vis_of<C: Component>(app: &mut App) -> Visibility {
    let mut q = app.world_mut().query_filtered::<&Visibility, With<C>>();
    *q.single(app.world()).unwrap()
}

/// Launch kicks and flashes like Fire, the chaingun cluster spins while it fires, and the
/// detonator shows only while bombs are live (the bomb in hand hides when none are left).
#[test]
fn new_weapon_models_react_to_events_and_bombs() {
    use rr_core::defs::AmmoKind;
    use rr_game::combat::PlayerArsenal;
    let mut app = app();
    app.update();
    app.world_mut()
        .resource_mut::<FxQueue>()
        .weapon
        .push(WeaponEvent::Launch {
            weapon: WeaponId::Rockets,
            dir: Vec3::X,
        });
    app.update();
    assert!(visible_flashes(&mut app) > 0, "flash after Launch");
    assert!(app.world().resource::<ViewState>().recoil.z > 0.0);

    app.world_mut()
        .resource_mut::<FxQueue>()
        .weapon
        .push(WeaponEvent::Fire {
            weapon: WeaponId::Chaingun,
            dirs: vec![Vec3::X],
        });
    app.update();
    let spin = |app: &mut App| {
        let mut q = app
            .world_mut()
            .query_filtered::<&Transform, With<BarrelCluster>>();
        q.single(app.world()).unwrap().rotation
    };
    let r0 = spin(&mut app);
    app.update();
    assert!(!r0.abs_diff_eq(spin(&mut app), 1e-9), "cluster turns");

    assert_eq!(vis_of::<Detonator>(&mut app), Visibility::Hidden);
    {
        let mut q = app.world_mut().query::<&mut PlayerArsenal>();
        let mut a = q.single_mut(app.world_mut()).unwrap();
        a.0.live_bombs = 2;
        a.0.reserve[AmmoKind::Bombs.index()] = 0;
        a.0.clip[WeaponId::PipeBombs.index()] = 0;
    }
    app.world_mut()
        .resource_mut::<FxQueue>()
        .weapon
        .push(WeaponEvent::Detonate);
    app.update();
    assert_eq!(vis_of::<Detonator>(&mut app), Visibility::Inherited);
    assert_eq!(vis_of::<HeldBomb>(&mut app), Visibility::Hidden);
    assert!(app.world().resource::<ViewState>().press > 0.0);
}
