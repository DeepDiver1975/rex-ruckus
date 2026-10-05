//! Carried utility items in the game: the medkit, jetpack and night-vision controls, and the
//! night-vision look (brighter ambient light plus a green screen tint).

use crate::combat::{PlayerInventory, PlayerVitals};
use crate::mechanics::HudMessage;
use crate::player::{PendingInput, Player};
use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use rr_core::inventory::{InvEvent, InvItem};

/// Ambient brightness multiplier while night vision is on.
pub const NV_BRIGHTNESS_SCALE: f32 = 6.0;
/// Tint of the night-vision screen overlay.
pub const NV_TINT: Color = Color::srgba(0.1, 1.0, 0.2, 0.14);
/// Below the HUD's own layers (the status bar and its text stay on top of the tint).
const NV_Z: i32 = -1;

/// Applies the latched Q, J and N taps through the core inventory, then drains the items.
/// Runs after `use_key` and before the player moves, so a jetpack toggled this tick flies
/// this tick.
pub fn use_inventory(
    time: Res<Time<Fixed>>,
    mut msg: ResMut<HudMessage>,
    mut q: Query<(&mut PendingInput, &mut PlayerVitals, &mut PlayerInventory), With<Player>>,
) {
    let dt = time.timestep().as_secs_f32();
    for (mut input, mut vitals, mut inv) in &mut q {
        if std::mem::take(&mut input.use_medkit) {
            inv.0.use_medkit(&mut vitals.0);
        }
        if std::mem::take(&mut input.toggle_jetpack) {
            inv.0.toggle(InvItem::Jetpack);
        }
        if std::mem::take(&mut input.toggle_nv) {
            inv.0.toggle(InvItem::NightVision);
        }
        for ev in inv.0.tick(dt) {
            msg.show(match ev {
                InvEvent::JetpackOff => "Jetpack out of fuel",
                InvEvent::NightVisionOff => "Night vision battery dead",
            });
        }
    }
}

/// The ambient brightness saved while night vision is on; `None` while it is off.
#[derive(Resource, Default, Debug)]
pub struct NightVision {
    base: Option<f32>,
}

#[derive(Component)]
struct NightVisionOverlay;

/// Night vision's look. Needs a renderer (`GlobalAmbientLight`, UI); not part of the sim.
pub struct NightVisionPlugin;

impl Plugin for NightVisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NightVision>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, apply_night_vision);
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(NV_TINT),
        GlobalZIndex(NV_Z),
        Visibility::Hidden,
        NightVisionOverlay,
    ));
}

/// Switches the look to match the player's `nv_on`, remembering the exact prior brightness.
fn apply_night_vision(
    player: Query<&PlayerInventory, With<Player>>,
    mut state: ResMut<NightVision>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut overlay: Single<&mut Visibility, With<NightVisionOverlay>>,
) {
    let on = player.single().is_ok_and(|i| i.0.nv_on);
    match (on, state.base) {
        (true, None) => {
            state.base = Some(ambient.brightness);
            ambient.brightness *= NV_BRIGHTNESS_SCALE;
            overlay.set_if_neq(Visibility::Inherited);
        }
        (false, Some(base)) => {
            ambient.brightness = base;
            state.base = None;
            overlay.set_if_neq(Visibility::Hidden);
        }
        _ => {}
    }
}

/// Puts the ambient light and the overlay back to their normal state (a level restart).
pub fn reset_night_vision(world: &mut World) {
    let base = world
        .get_resource_mut::<NightVision>()
        .and_then(|mut s| s.base.take());
    if let (Some(base), Some(mut ambient)) = (base, world.get_resource_mut::<GlobalAmbientLight>())
    {
        ambient.brightness = base;
    }
    let mut q = world.query_filtered::<&mut Visibility, With<NightVisionOverlay>>();
    for mut vis in q.iter_mut(world) {
        vis.set_if_neq(Visibility::Hidden);
    }
}
