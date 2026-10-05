//! The Enforcer: a bulky dark-blue armoured trooper with wide shoulders and a long shotgun.

use super::{GunnerMats, GunnerRig, Skin, emissive, matte, part, pivot};
use bevy::prelude::*;

/// Height of the Enforcer model as built; the root is scaled to the enemy def's height.
pub(super) const MODEL_HEIGHT: f32 = 1.85;
/// Half the torso depth.
const BACK_HALF_DEPTH: f32 = 0.18;

/// Marks the root of an Enforcer's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct EnforcerVisual(pub usize);

#[derive(Clone)]
pub(super) struct Assets {
    legs: Handle<Mesh>,
    torso: Handle<Mesh>,
    shoulders: Handle<Mesh>,
    head: Handle<Mesh>,
    visor: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    gun: Handle<Mesh>,
    tip: Handle<Mesh>,
    gun_mat: Handle<StandardMaterial>,
    mats: GunnerMats,
}

impl Assets {
    pub(super) fn new(
        meshes: &mut bevy::prelude::Assets<Mesh>,
        materials: &mut bevy::prelude::Assets<StandardMaterial>,
    ) -> Self {
        Assets {
            legs: meshes.add(Cuboid::new(0.5, 0.9, 0.3)),
            torso: meshes.add(Cuboid::new(0.78, 0.7, 2.0 * BACK_HALF_DEPTH)),
            shoulders: meshes.add(Cuboid::new(1.08, 0.22, 0.42)),
            head: meshes.add(Cuboid::new(0.3, 0.28, 0.3)),
            visor: meshes.add(Cuboid::new(0.26, 0.07, 0.04)),
            upper_arm: meshes.add(Cuboid::new(0.18, 0.6, 0.18)),
            gun: meshes.add(Cuboid::new(0.12, 0.14, 1.0)),
            tip: meshes.add(Sphere::new(0.07)),
            gun_mat: materials.add(matte(Color::srgb(0.12, 0.12, 0.14))),
            mats: GunnerMats {
                skin: Skin::new(materials, [0.12, 0.2, 0.5]),
                visor_on: materials.add(emissive(Color::srgb(1.0, 0.75, 0.1), 4.0)),
                visor_off: materials.add(matte(Color::srgb(0.1, 0.08, 0.03))),
                tip_idle: materials.add(matte(Color::srgb(0.25, 0.12, 0.05))),
                tip_glow: materials.add(emissive(Color::srgb(1.0, 0.7, 0.2), 8.0)),
            },
        }
    }
}

/// Builds the Enforcer's parts under `root` (same model space as the Grunt).
pub(super) fn spawn(commands: &mut Commands, root: Entity, index: usize, a: &Assets) {
    let skin = &a.mats.skin.normal;
    let legs = part(commands, root, &a.legs, skin, Vec3::Y * 0.45);
    let torso = part(commands, root, &a.torso, skin, Vec3::Y * 1.25);
    let shoulders = part(commands, root, &a.shoulders, skin, Vec3::Y * 1.56);
    let left_arm = part(
        commands,
        root,
        &a.upper_arm,
        skin,
        Vec3::new(-0.5, 1.2, 0.0),
    );
    let neck = pivot(commands, root, Vec3::Y * 1.6);
    let head = part(commands, neck, &a.head, skin, Vec3::Y * 0.1);
    let visor = part(
        commands,
        neck,
        &a.visor,
        &a.mats.visor_on,
        Vec3::new(0.0, 0.12, -0.16),
    );
    let arm = pivot(commands, root, Vec3::new(0.5, 1.4, 0.0));
    part(commands, arm, &a.gun, &a.gun_mat, Vec3::Z * -0.5);
    let tip = part(commands, arm, &a.tip, &a.mats.tip_idle, Vec3::Z * -1.03);
    commands.entity(root).insert((
        EnforcerVisual(index),
        GunnerRig {
            skin: vec![legs, torso, shoulders, left_arm, head],
            neck,
            visor,
            arm,
            tip,
            back_half_depth: BACK_HALF_DEPTH,
            mats: a.mats.clone(),
        },
    ));
}
