//! The Slasher: a lean, hunched red melee enemy with two blade arms and no gun.

use super::{
    ActorVisual, HitFlash, Skin, SkinLook, emissive, matte, part, pivot, place_walker, pose,
    set_material, skin_look,
};
use crate::combat::{GameDefs, LevelCombat};
use bevy::prelude::*;
use rr_core::actors::AiState;

/// Height of the Slasher model as built; the root is scaled to the enemy def's height.
pub(super) const MODEL_HEIGHT: f32 = 1.5;
/// Half the torso depth.
const BACK_HALF_DEPTH: f32 = 0.11;
/// Forward lean of the spine (radians).
const HUNCH: f32 = 0.55;

/// Marks the root of a Slasher's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct SlasherVisual(pub usize);

/// The animated parts of a Slasher.
#[derive(Component)]
pub(super) struct SlasherRig {
    skin: [Entity; 5],
    neck: Entity,
    eyes: Entity,
    arms: [Entity; 2],
    mats: Mats,
}

#[derive(Clone)]
struct Mats {
    skin: Skin,
    eyes_on: Handle<StandardMaterial>,
    eyes_off: Handle<StandardMaterial>,
}

#[derive(Clone)]
pub(super) struct Assets {
    legs: Handle<Mesh>,
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    eyes: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    blade: Handle<Mesh>,
    blade_mat: Handle<StandardMaterial>,
    mats: Mats,
}

impl Assets {
    pub(super) fn new(
        meshes: &mut bevy::prelude::Assets<Mesh>,
        materials: &mut bevy::prelude::Assets<StandardMaterial>,
    ) -> Self {
        Assets {
            legs: meshes.add(Cuboid::new(0.3, 0.7, 0.2)),
            torso: meshes.add(Cuboid::new(0.4, 0.62, 2.0 * BACK_HALF_DEPTH)),
            head: meshes.add(Cuboid::new(0.24, 0.24, 0.26)),
            eyes: meshes.add(Cuboid::new(0.2, 0.04, 0.04)),
            upper_arm: meshes.add(Cuboid::new(0.1, 0.1, 0.35)),
            blade: meshes.add(Cuboid::new(0.04, 0.14, 0.7)),
            blade_mat: materials.add(emissive(Color::srgb(0.75, 0.8, 0.85), 0.6)),
            mats: Mats {
                skin: Skin::new(materials, [0.62, 0.08, 0.08]),
                eyes_on: materials.add(emissive(Color::srgb(1.0, 0.9, 0.1), 5.0)),
                eyes_off: materials.add(matte(Color::srgb(0.1, 0.08, 0.02))),
            },
        }
    }
}

/// Builds the Slasher's parts under `root` (same model space as the Grunt). The torso, head and
/// arms hang off a spine pivot leaning forward; each arm ends in a long blade.
pub(super) fn spawn(commands: &mut Commands, root: Entity, index: usize, a: &Assets) {
    let skin = &a.mats.skin.normal;
    let legs = part(commands, root, &a.legs, skin, Vec3::Y * 0.35);
    let spine = pivot(commands, root, Vec3::Y * 0.7);
    commands
        .entity(spine)
        .insert(Transform::from_xyz(0.0, 0.7, 0.0).with_rotation(Quat::from_rotation_x(-HUNCH)));
    let torso = part(commands, spine, &a.torso, skin, Vec3::Y * 0.3);
    let neck = pivot(commands, spine, Vec3::Y * 0.62);
    let head = part(commands, neck, &a.head, skin, Vec3::Y * 0.12);
    let eyes = part(
        commands,
        neck,
        &a.eyes,
        &a.mats.eyes_on,
        Vec3::new(0.0, 0.14, -0.14),
    );
    let mut arms = [Entity::PLACEHOLDER; 2];
    let mut upper = [Entity::PLACEHOLDER; 2];
    for (k, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
        let arm = pivot(commands, spine, Vec3::new(0.28 * side, 0.5, 0.0));
        upper[k] = part(commands, arm, &a.upper_arm, skin, Vec3::Z * -0.17);
        part(commands, arm, &a.blade, &a.blade_mat, Vec3::Z * -0.65);
        arms[k] = arm;
    }
    commands.entity(root).insert((
        SlasherVisual(index),
        SlasherRig {
            skin: [legs, torso, head, upper[0], upper[1]],
            neck,
            eyes,
            arms,
            mats: a.mats.clone(),
        },
    ));
}

/// Blade-arm pitch for an AI state: 0 = thrust straight forward, negative = hanging low.
/// During `Attack` (the lunge) both blades swing forward.
fn arm_angle(state: AiState) -> f32 {
    match state {
        AiState::Attack { .. } => 0.1,
        AiState::Sleep | AiState::Dying { .. } | AiState::Dead => -1.3,
        AiState::Pain { .. } => -0.4,
        AiState::Alert { .. } | AiState::Chase => -0.9,
    }
}

/// Places every Slasher between its last two ticks and poses it from its AI state.
pub(super) fn pose_slashers(
    fixed: Res<Time<Fixed>>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    mut roots: Query<(&ActorVisual, &SlasherRig, &HitFlash, &mut Transform)>,
    mut parts: Query<&mut Transform, Without<ActorVisual>>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, rig, hit, mut t) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let p = pose(actor, defs.0.enemy(actor.kind).death_time);
        place_walker(&mut t, actor, alpha, p.pitch, BACK_HALF_DEPTH);
        if let Ok(mut n) = parts.get_mut(rig.neck) {
            n.rotation = Quat::from_rotation_x(p.nod);
        }
        for arm in rig.arms {
            if let Ok(mut t) = parts.get_mut(arm) {
                t.rotation = Quat::from_rotation_x(arm_angle(actor.state));
            }
        }
        let look: SkinLook = skin_look(p.skin, actor.alive(), hit.0);
        for e in rig.skin {
            set_material(&mut mats, e, rig.mats.skin.pick(look));
        }
        let eyes = if p.visor_lit {
            &rig.mats.eyes_on
        } else {
            &rig.mats.eyes_off
        };
        set_material(&mut mats, rig.eyes, eyes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blades_swing_forward_only_in_the_lunge() {
        let lunge = arm_angle(AiState::Attack { t: 0.0, left: 1 });
        for s in [AiState::Chase, AiState::Sleep, AiState::Pain { t: 0.1 }] {
            assert!(lunge > arm_angle(s), "{s:?}");
        }
    }
}
