//! The Drone: a floating purple disc with a ring and a glowing eye; no legs, gentle bob.

use super::{
    ActorVisual, HitFlash, Skin, SkinLook, death_pitch, emissive, matte, part, pivot, set_material,
    skin_look,
};
use crate::combat::{GameDefs, LevelCombat};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::level::CurrentMap;
use bevy::prelude::*;
use rr_core::actors::AiState;
use std::f32::consts::FRAC_PI_2;

/// Height of the Drone model as built (matches its def); the root's scale is 1.
pub(super) const MODEL_HEIGHT: f32 = 0.6;
/// Bob amplitude (m, model space) and angular speed (rad/s).
const BOB_AMP: f32 = 0.05;
const BOB_SPEED: f32 = 3.0;
/// Roll of a wreck lying on the floor (radians).
const WRECK_ROLL: f32 = 0.5;

/// Marks the root of a Drone's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct DroneVisual(pub usize);

/// The animated parts of a Drone.
#[derive(Component)]
pub(super) struct DroneRig {
    /// Bobbing pivot holding hull, ring and eye.
    hull: Entity,
    skin: [Entity; 2],
    eye: Entity,
    mats: Mats,
}

#[derive(Clone)]
struct Mats {
    skin: Skin,
    eye_on: Handle<StandardMaterial>,
    eye_off: Handle<StandardMaterial>,
}

#[derive(Clone)]
pub(super) struct Assets {
    hull: Handle<Mesh>,
    ring: Handle<Mesh>,
    eye: Handle<Mesh>,
    ring_mat: Handle<StandardMaterial>,
    mats: Mats,
}

impl Assets {
    pub(super) fn new(
        meshes: &mut bevy::prelude::Assets<Mesh>,
        materials: &mut bevy::prelude::Assets<StandardMaterial>,
    ) -> Self {
        Assets {
            hull: meshes.add(Sphere::new(0.3)),
            ring: meshes.add(Torus::new(0.34, 0.42)),
            eye: meshes.add(Sphere::new(0.09)),
            ring_mat: materials.add(matte(Color::srgb(0.7, 0.7, 0.75))),
            mats: Mats {
                skin: Skin::new(materials, [0.38, 0.2, 0.55]),
                eye_on: materials.add(emissive(Color::srgb(0.2, 1.0, 0.9), 6.0)),
                eye_off: materials.add(matte(Color::srgb(0.05, 0.1, 0.1))),
            },
        }
    }
}

/// Builds the Drone under `root`: the origin is the body's feet (its hover height), so the
/// hull's centre is half the model height up.
pub(super) fn spawn(commands: &mut Commands, root: Entity, index: usize, a: &Assets) {
    let hull_pivot = pivot(commands, root, Vec3::Y * 0.3);
    let hull = part(
        commands,
        hull_pivot,
        &a.hull,
        &a.mats.skin.normal,
        Vec3::ZERO,
    );
    // A flattened sphere reads as a disc.
    commands
        .entity(hull)
        .insert(Transform::from_scale(Vec3::new(1.2, 0.7, 1.2)));
    let ring = part(commands, hull_pivot, &a.ring, &a.ring_mat, Vec3::ZERO);
    let eye = part(
        commands,
        hull_pivot,
        &a.eye,
        &a.mats.eye_on,
        Vec3::new(0.0, 0.02, -0.34),
    );
    commands.entity(root).insert((
        DroneVisual(index),
        DroneRig {
            hull: hull_pivot,
            skin: [hull, ring],
            eye,
            mats: a.mats.clone(),
        },
    ));
}

/// How far a dying drone has dropped, 0 (hovering) to 1 (on the floor).
fn drop_progress(state: AiState, death_time: f32) -> f32 {
    match state {
        AiState::Dying { t } => death_pitch(t, death_time) / FRAC_PI_2,
        AiState::Dead => 1.0,
        _ => 0.0,
    }
}

/// Places every Drone between its last two ticks, bobbing; a dying one drops to the floor
/// and lies tilted.
#[allow(clippy::too_many_arguments)]
pub(super) fn pose_drones(
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    map: Res<CurrentMap>,
    mut roots: Query<(&ActorVisual, &DroneRig, &HitFlash, &mut Transform)>,
    mut parts: Query<&mut Transform, Without<ActorVisual>>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, rig, hit, mut t) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let def = defs.0.enemy(actor.kind);
        let done = drop_progress(actor.state, def.death_time);
        let feet = actor.prev_pos.lerp(actor.body.pos, alpha);
        let floor = map.0.sectors[actor.body.sector].floor_z;
        let drop = (feet.z - floor).max(0.0) * done;
        t.translation = to_bevy(feet) - Vec3::Y * drop;
        t.rotation = Quat::from_rotation_y(core_angle_to_yaw(actor.angle))
            * Quat::from_rotation_z(WRECK_ROLL * done);
        if let Ok(mut h) = parts.get_mut(rig.hull) {
            let bob = if actor.alive() {
                (time.elapsed_secs() * BOB_SPEED + g.0 as f32).sin() * BOB_AMP
            } else {
                0.0
            };
            h.translation.y = 0.3 + bob;
        }
        let base = match actor.state {
            AiState::Pain { .. } => SkinLook::Pain,
            _ => SkinLook::Normal,
        };
        let look = skin_look(base, actor.alive(), hit.0);
        for e in rig.skin {
            set_material(&mut mats, e, rig.mats.skin.pick(look));
        }
        let eye = if actor.alive() {
            &rig.mats.eye_on
        } else {
            &rig.mats.eye_off
        };
        set_material(&mut mats, rig.eye, eye);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dying_drone_drops_monotonically_to_the_floor() {
        assert_eq!(drop_progress(AiState::Chase, 0.6), 0.0);
        assert_eq!(drop_progress(AiState::Dead, 0.6), 1.0);
        let mut last = -1.0;
        for i in 0..=10 {
            let t = 0.6 * (1.0 - i as f32 / 10.0);
            let d = drop_progress(AiState::Dying { t }, 0.6);
            assert!(d >= last && (0.0..=1.0).contains(&d));
            last = d;
        }
    }
}
