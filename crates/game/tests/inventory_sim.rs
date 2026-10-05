//! Inventory controls (Q, J, N), night vision and the HUD's change detection.

use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::fixtures::combat_room;
use rr_game::combat::{CombatSimPlugin, PlayerInventory, PlayerVitals, insert_defs};
use rr_game::flow::FlowPlugin;
use rr_game::hud::HudPlugin;
use rr_game::inventory::NightVisionPlugin;
use rr_game::mechanics::{HudMessage, MechanicsSimPlugin, insert_level};
use rr_game::player::{PendingInput, PlayerBody, PlayerSimPlugin};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, combat_room());
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
        NightVisionPlugin,
        HudPlugin,
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

fn carried(app: &mut App) -> Mut<'_, PlayerInventory> {
    let mut q = app.world_mut().query::<&mut PlayerInventory>();
    q.single_mut(app.world_mut()).unwrap()
}

fn vitals(app: &mut App) -> Mut<'_, PlayerVitals> {
    let mut q = app.world_mut().query::<&mut PlayerVitals>();
    q.single_mut(app.world_mut()).unwrap()
}

fn feet_z(app: &mut App) -> f32 {
    let mut q = app.world_mut().query::<&PlayerBody>();
    q.single(app.world()).unwrap().0.pos.z
}

fn brightness(app: &App) -> f32 {
    app.world().resource::<GlobalAmbientLight>().brightness
}

fn overlay_visible(app: &mut App) -> bool {
    let mut q = app
        .world_mut()
        .query_filtered::<(&Visibility, &GlobalZIndex), Without<Text>>();
    q.iter(app.world())
        .any(|(v, z)| z.0 < 0 && *v != Visibility::Hidden)
}

#[test]
fn medkit_key_heals() {
    let mut app = app();
    vitals(&mut app).0.health.hp = 50;
    carried(&mut app).0.medkit = 30;
    input(&mut app).use_medkit = true;
    ticks(&mut app, 1);
    assert_eq!(vitals(&mut app).0.health.hp, 80);
    assert_eq!(carried(&mut app).0.medkit, 0);
    assert!(!input(&mut app).use_medkit, "latch consumed");
}

#[test]
fn jetpack_key_toggles_flight() {
    let mut app = app();
    carried(&mut app).0.fuel = 100.0;
    input(&mut app).toggle_jetpack = true;
    ticks(&mut app, 1);
    assert!(carried(&mut app).0.jetpack_on);
    input(&mut app).jump = true;
    ticks(&mut app, 30);
    let up = feet_z(&mut app);
    assert!(up > 1.0, "thrust lifts the player: {up}");
    assert!(carried(&mut app).0.fuel < 100.0, "flying burns fuel");

    input(&mut app).toggle_jetpack = true;
    input(&mut app).jump = false;
    ticks(&mut app, 60);
    assert!(!carried(&mut app).0.jetpack_on);
    assert!(feet_z(&mut app) < up, "falls again once it is off");
}

#[test]
fn empty_jetpack_does_not_turn_on() {
    let mut app = app();
    input(&mut app).toggle_jetpack = true;
    ticks(&mut app, 1);
    assert!(!carried(&mut app).0.jetpack_on);
}

#[test]
fn running_dry_switches_off_and_says_so() {
    let mut app = app();
    {
        let mut c = carried(&mut app);
        c.0.fuel = 0.01;
        c.0.battery = 0.01;
    }
    input(&mut app).toggle_jetpack = true;
    input(&mut app).toggle_nv = true;
    ticks(&mut app, 2);
    let c = carried(&mut app).0;
    assert!(!c.jetpack_on && !c.nv_on);
    let msg = app.world().resource::<HudMessage>().text.clone();
    assert!(msg.contains("battery") || msg.contains("fuel"), "{msg}");
}

#[test]
fn nightvision_toggles_ambient() {
    let mut app = app();
    carried(&mut app).0.battery = 100.0;
    assert!(!overlay_visible(&mut app));
    input(&mut app).toggle_nv = true;
    ticks(&mut app, 1);
    app.update();
    assert_eq!(brightness(&app), 1500.0);
    assert!(overlay_visible(&mut app));
    app.update();
    assert_eq!(brightness(&app), 1500.0, "not multiplied twice");

    input(&mut app).toggle_nv = true;
    ticks(&mut app, 1);
    app.update();
    assert_eq!(brightness(&app), 250.0, "exact prior value restored");
    assert!(!overlay_visible(&mut app));
}

#[test]
fn hud_text_unchanged_not_marked_changed() {
    let mut app = app();
    app.update();
    app.update();
    let mut q = app.world_mut().query::<Ref<Text>>();
    let texts: Vec<bool> = q.iter(app.world()).map(|t| t.is_changed()).collect();
    assert!(!texts.is_empty());
    assert!(
        texts.iter().all(|c| !c),
        "no HUD text may be rewritten without a state change"
    );
}
