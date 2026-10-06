//! The Drone: a hovering gun robot on a glTF model with a gentle bob; a dying one rolls onto
//! its side while core's gravity brings it down and its Dead clip plays.

use super::{ActorModel, ActorVisual, death_pitch};
use crate::combat::{GameDefs, LevelCombat};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::models::ModelLibrary;
use bevy::prelude::*;
use rr_core::actors::AiState;
use rr_core::map::ActorKind;
use std::f32::consts::FRAC_PI_2;

/// Bob amplitude (m) and angular speed (rad/s).
const BOB_AMP: f32 = 0.05;
const BOB_SPEED: f32 = 3.0;
/// Roll of a wreck lying on the floor (radians).
const WRECK_ROLL: f32 = 0.5;

/// Marks the root of a Drone's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct DroneVisual(pub usize);

/// How far a dying drone has rolled onto its side, 0 (upright) to 1 (wreck).
fn wreck_progress(state: AiState, death_time: f32) -> f32 {
    match state {
        AiState::Dying { t } => death_pitch(t, death_time) / FRAC_PI_2,
        AiState::Dead => 1.0,
        _ => 0.0,
    }
}

/// Places every Drone between its last two ticks (its feet are the hover height), rolling a
/// dying one; the model bobs while alive.
pub(super) fn place_drones(
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    lib: Option<Res<ModelLibrary>>,
    mut roots: Query<(&ActorVisual, Option<&ActorModel>, &mut Transform), With<DroneVisual>>,
    mut models: Query<&mut Transform, Without<ActorVisual>>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, model, mut t) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let done = wreck_progress(actor.state, defs.0.enemy(actor.kind).death_time);
        let feet = actor.prev_pos.lerp(actor.body.pos, alpha);
        // Core already drops a dead flyer to the floor, so `feet` falls by itself.
        t.translation = to_bevy(feet);
        t.rotation = Quat::from_rotation_y(core_angle_to_yaw(actor.angle))
            * Quat::from_rotation_z(WRECK_ROLL * done);
        if let (Some(model), Some(lib)) = (model, lib.as_deref())
            && let Ok(mut m) = models.get_mut(model.root)
        {
            let bob = if actor.alive() {
                (time.elapsed_secs() * BOB_SPEED + g.0 as f32).sin() * BOB_AMP
            } else {
                0.0
            };
            m.translation.y = lib.enemy(ActorKind::Drone).model.place.offset.1 + bob;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dying_drone_rolls_monotonically() {
        assert_eq!(wreck_progress(AiState::Chase, 0.6), 0.0);
        assert_eq!(wreck_progress(AiState::Dead, 0.6), 1.0);
        let mut last = -1.0;
        for i in 0..=10 {
            let t = 0.6 * (1.0 - i as f32 / 10.0);
            let d = wreck_progress(AiState::Dying { t }, 0.6);
            assert!(d >= last && (0.0..=1.0).contains(&d));
            last = d;
        }
    }
}
