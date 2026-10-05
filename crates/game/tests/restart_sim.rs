//! A restart resets everything the M4a arsenal added: destructibles, inventory, armour,
//! effects, decals, live bombs and the carried-over input latches.

use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::map::Map;
use rr_core::mechanics::Motion;
use rr_core::projectile::{Projectile, Shooter, Targets};
use rr_game::breakables::{BreakablesPlugin, LightFixture};
use rr_game::combat::{
    CombatSimPlugin, FxQueue, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
    insert_defs,
};
use rr_game::decals::{Decal, DecalRing, DecalsPlugin};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::fx::{FxPlugin, ScreenShake};
use rr_game::inventory::NightVisionPlugin;
use rr_game::level::{CurrentMap, LevelLight, LevelRenderPlugin};
use rr_game::mechanics::{LevelMechanics, MechanicsSimPlugin, insert_level};
use rr_game::player::{PendingInput, PlayerSimPlugin};
use std::time::Duration;

/// A 10×10 m hall (sector 0) with a glass-fronted booth east (sector 1), a crack wall slab north
/// (sector 2) in front of a secret closet (sector 3), and one breakable light.
fn destructible_level() -> Map {
    Map::from_ron(
        r#"(
        name: "destructibles",
        materials: ["wall", "floor", "ceiling"],
        vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (10.0, 6.0), (10.0, 10.0), (0.0, 10.0),
                   (14.0, 4.0), (14.0, 6.0),
                   (10.0, 10.5), (0.0, 10.5), (10.0, 12.0), (0.0, 12.0)],
        sectors: [
            (loops: [[0, 1, 2, 3, 4, 5]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[2, 6, 7, 3]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[5, 4, 8, 9]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0,
             mover: Some((kind: Crack))),
            (loops: [[9, 8, 10, 11]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0,
             secret: true),
        ],
        player_start: (pos: (5.0, 5.0), angle_deg: 0.0),
        glass: [(2, 3)],
        lights: [(pos: (5.0, 5.0, 3.5), color: (1.0, 1.0, 1.0), intensity: 800.0, range: 8.0, breakable: true)],
    )"#,
    )
    .expect("destructible level is valid")
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    insert_level(&mut app, destructible_level());
    insert_defs(&mut app, Defs::builtin());
    app.insert_resource(GlobalAmbientLight {
        brightness: 250.0,
        ..default()
    })
    .add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        LevelRenderPlugin,
        DecalsPlugin,
        BreakablesPlugin,
        FxPlugin,
        NightVisionPlugin,
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

fn vitals(app: &mut App) -> Mut<'_, PlayerVitals> {
    let mut q = app.world_mut().query::<&mut PlayerVitals>();
    q.single_mut(app.world_mut()).unwrap()
}

fn carried(app: &mut App) -> Mut<'_, PlayerInventory> {
    let mut q = app.world_mut().query::<&mut PlayerInventory>();
    q.single_mut(app.world_mut()).unwrap()
}

fn kill_player(app: &mut App) {
    vitals(app).0.damage(1000);
    ticks(app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
}

/// Waits out the death screen's grace period, then presses use.
fn restart(app: &mut App) {
    ticks(app, 70);
    input(app).use_pressed = true;
    ticks(app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<(), F>();
    q.iter(app.world()).count()
}

#[test]
fn restart_restores_destructibles() {
    let mut app = app();
    let lights = count::<With<LevelLight>>(&mut app);
    assert_eq!(lights, 1);
    // Break it all through the real combat state.
    let pane = app
        .world()
        .resource::<CurrentMap>()
        .0
        .walls
        .iter()
        .position(|w| w.glass)
        .unwrap();
    let world = app.world_mut();
    let mut combat = world.remove_resource::<LevelCombat>().unwrap();
    let mut map = world.remove_resource::<CurrentMap>().unwrap();
    let mut mech = world.remove_resource::<LevelMechanics>().unwrap();
    assert!(!combat.0.destruct.break_glass(&mut map.0, pane).is_empty());
    assert!(combat.0.destruct.open_crack(&mut mech.0, 2));
    assert!(combat.0.destruct.break_light(&map.0, 0));
    assert!(combat.0.destruct.enter_sector(&map.0, 3));
    assert_eq!(combat.0.destruct.secrets(), (1, 1));
    world.insert_resource(combat);
    world.insert_resource(map);
    world.insert_resource(mech);
    world
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::LightBroken(0));
    app.update();
    assert!(
        count::<With<LightFixture>>(&mut app) > 0
            && app
                .world()
                .resource::<LevelCombat>()
                .0
                .destruct
                .light_broken(0)
    );

    kill_player(&mut app);
    restart(&mut app);

    let world = app.world();
    assert!(
        world
            .resource::<CurrentMap>()
            .0
            .walls
            .iter()
            .any(|w| w.glass),
        "the pane is back"
    );
    let mech = &world.resource::<LevelMechanics>().0;
    assert_eq!(mech.movers[0].motion, Motion::AtStart, "crack sealed again");
    let destruct = &world.resource::<LevelCombat>().0.destruct;
    assert!(!destruct.light_broken(0), "light intact");
    assert_eq!(destruct.secrets(), (0, 1), "secret not found");
    assert_eq!(count::<With<LevelLight>>(&mut app), lights, "light is back");
}

#[test]
fn restart_resets_inventory_and_armour() {
    let mut app = app();
    {
        let mut v = vitals(&mut app);
        v.0.add_armour(80);
        v.0.damage(30);
    }
    {
        let mut c = carried(&mut app);
        c.0.medkit = 70;
        c.0.fuel = 40.0;
        c.0.battery = 25.0;
        c.0.jetpack_on = true;
        c.0.nv_on = true;
    }
    app.world_mut().resource_mut::<ScreenShake>().trauma = 0.9;
    kill_player(&mut app);
    restart(&mut app);

    let v = vitals(&mut app).0;
    assert_eq!((v.health.hp, v.armour), (100, 0));
    assert_eq!(
        carried(&mut app).0,
        rr_core::inventory::Inventory::default()
    );
    assert_eq!(app.world().resource::<ScreenShake>().trauma, 0.0);
}

#[test]
fn restart_clears_decals_and_bombs() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<FxQueue>()
        .combat
        .push(CombatEvent::Impact {
            point: Vec3::new(1.0, 1.0, 1.0),
            normal: Vec3::X,
            sector: 0,
            wall: None,
        });
    {
        let mut combat = app.world_mut().resource_mut::<LevelCombat>();
        let pos = Vec3::new(6.0, 5.0, 0.2);
        combat.0.projectiles.push(Projectile {
            id: 900,
            pos,
            prev: pos,
            vel: Vec3::ZERO,
            sector: 0,
            radius: 0.15,
            damage: 0,
            owner: Shooter::Player,
            targets: Targets::All,
            life: 4.0,
            gravity: 9.8,
            bounce: Some(0.3),
            remote: true,
            splash: None,
            resting: true,
        });
    }
    app.update();
    assert_eq!(count::<With<Decal>>(&mut app), 1);
    assert_eq!(app.world().resource::<LevelCombat>().0.projectiles.len(), 1);
    app.world_mut().resource_mut::<ScreenShake>().trauma = 0.5;

    kill_player(&mut app);
    restart(&mut app);

    assert_eq!(count::<With<Decal>>(&mut app), 0);
    assert_eq!(app.world().resource::<DecalRing>().len(), 0);
    assert_eq!(
        app.world().resource::<ScreenShake>().trauma,
        0.0,
        "shake calmed"
    );
    let combat = &app.world().resource::<LevelCombat>().0;
    assert!(combat.projectiles.is_empty() && combat.pending_blasts.is_empty());
    let arsenal = &mut app.world_mut().query::<&PlayerArsenal>();
    assert_eq!(arsenal.single(app.world()).unwrap().0.live_bombs, 0);
}

#[test]
fn restart_clears_inventory_latches() {
    let mut app = app();
    kill_player(&mut app);
    // Pressed while dead: nothing consumes these until the restart.
    {
        let mut i = input(&mut app);
        i.use_medkit = true;
        i.toggle_jetpack = true;
        i.toggle_nv = true;
    }
    restart(&mut app);
    {
        let i = input(&mut app);
        assert!(!i.use_medkit && !i.toggle_jetpack && !i.toggle_nv);
    }
    // And they cannot fire on the first tick of the new run either.
    {
        let mut c = carried(&mut app);
        c.0.fuel = 50.0;
        c.0.battery = 50.0;
        c.0.medkit = 50;
    }
    vitals(&mut app).0.health.hp = 40;
    ticks(&mut app, 1);
    let c = carried(&mut app).0;
    assert!(!c.jetpack_on && !c.nv_on);
    assert_eq!(c.medkit, 50, "no medkit was used");
}

#[test]
fn restart_restores_night_vision() {
    let mut app = app();
    {
        let mut c = carried(&mut app);
        c.0.battery = 100.0;
        c.0.nv_on = true;
    }
    app.update();
    assert_eq!(
        app.world().resource::<GlobalAmbientLight>().brightness,
        1500.0
    );
    kill_player(&mut app);
    restart(&mut app);
    assert_eq!(
        app.world().resource::<GlobalAmbientLight>().brightness,
        250.0,
        "ambient light back to normal immediately"
    );
    let mut q = app
        .world_mut()
        .query_filtered::<(&Visibility, &GlobalZIndex), Without<Text>>();
    assert!(
        q.iter(app.world())
            .all(|(v, z)| z.0 >= 0 || *v == Visibility::Hidden)
    );
}
