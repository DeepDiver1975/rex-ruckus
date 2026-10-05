//! The Grunt: an olive-green foot soldier with a red visor and a pistol-sized gun.

use super::{GunnerMats, GunnerRig, Skin, emissive, matte, part, pivot};
use bevy::prelude::*;

/// Height of the Grunt model as built; the root is scaled to the enemy def's height.
pub(super) const MODEL_HEIGHT: f32 = 1.75;
/// Half the torso depth.
const BACK_HALF_DEPTH: f32 = 0.15;

/// Marks the root of a Grunt's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct GruntVisual(pub usize);

#[derive(Clone)]
pub(super) struct Assets {
    legs: Handle<Mesh>,
    torso: Handle<Mesh>,
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
            legs: meshes.add(Cuboid::new(0.36, 0.8, 0.22)),
            torso: meshes.add(Cuboid::new(0.52, 0.62, 2.0 * BACK_HALF_DEPTH)),
            head: meshes.add(Cuboid::new(0.26, 0.28, 0.26)),
            visor: meshes.add(Cuboid::new(0.2, 0.06, 0.04)),
            upper_arm: meshes.add(Cuboid::new(0.12, 0.55, 0.12)),
            gun: meshes.add(Cuboid::new(0.12, 0.12, 0.6)),
            tip: meshes.add(Sphere::new(0.06)),
            gun_mat: materials.add(matte(Color::srgb(0.2, 0.2, 0.22))),
            mats: GunnerMats {
                skin: Skin::new(materials, [0.35, 0.42, 0.22]),
                visor_on: materials.add(emissive(Color::srgb(1.0, 0.1, 0.05), 4.0)),
                visor_off: materials.add(matte(Color::srgb(0.1, 0.05, 0.05))),
                tip_idle: materials.add(matte(Color::srgb(0.3, 0.12, 0.05))),
                tip_glow: materials.add(emissive(Color::srgb(1.0, 0.6, 0.15), 8.0)),
            },
        }
    }
}

/// Builds the Grunt's parts under `root`. Model space: feet at the origin, y up, facing −Z
/// (right hand on +X).
pub(super) fn spawn(commands: &mut Commands, root: Entity, index: usize, a: &Assets) {
    let skin = &a.mats.skin.normal;
    let legs = part(commands, root, &a.legs, skin, Vec3::Y * 0.4);
    let torso = part(commands, root, &a.torso, skin, Vec3::Y * 1.11);
    let left_arm = part(
        commands,
        root,
        &a.upper_arm,
        skin,
        Vec3::new(-0.33, 1.1, 0.0),
    );
    let neck = pivot(commands, root, Vec3::Y * 1.42);
    let head = part(commands, neck, &a.head, skin, Vec3::Y * 0.14);
    let visor = part(
        commands,
        neck,
        &a.visor,
        &a.mats.visor_on,
        Vec3::new(0.0, 0.16, -0.14),
    );
    let arm = pivot(commands, root, Vec3::new(0.33, 1.32, 0.0));
    part(commands, arm, &a.gun, &a.gun_mat, Vec3::Z * -0.3);
    let tip = part(commands, arm, &a.tip, &a.mats.tip_idle, Vec3::Z * -0.62);
    commands.entity(root).insert((
        GruntVisual(index),
        GunnerRig {
            skin: vec![legs, torso, left_arm, head],
            neck,
            visor,
            arm,
            tip,
            back_half_depth: BACK_HALF_DEPTH,
            mats: a.mats.clone(),
        },
    ));
}
