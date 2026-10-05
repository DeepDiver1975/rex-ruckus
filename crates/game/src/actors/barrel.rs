//! The explosive barrel: a red cylinder with a yellow band. It never animates and simply
//! disappears when it dies; its explosion effect covers the moment.

use super::{ActorVisual, HitFlash, matte, part};
use crate::combat::LevelCombat;
use bevy::prelude::*;

/// Height of the barrel model as built; the root is scaled to the enemy def's height.
pub(super) const MODEL_HEIGHT: f32 = 1.0;

/// Marks the root of a barrel's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct BarrelVisual(pub usize);

/// The body part, which takes the hit flash.
#[derive(Component)]
pub(super) struct BarrelRig {
    body: Entity,
}

#[derive(Clone)]
pub(super) struct Assets {
    body: Handle<Mesh>,
    band: Handle<Mesh>,
    red: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
    yellow: Handle<StandardMaterial>,
}

impl Assets {
    pub(super) fn new(
        meshes: &mut bevy::prelude::Assets<Mesh>,
        materials: &mut bevy::prelude::Assets<StandardMaterial>,
    ) -> Self {
        Assets {
            body: meshes.add(Cylinder::new(0.4, 1.0)),
            band: meshes.add(Cylinder::new(0.41, 0.16)),
            red: materials.add(matte(Color::srgb(0.7, 0.08, 0.06))),
            flash: materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.85, 0.7),
                emissive: Color::srgb(1.0, 0.85, 0.7).to_linear() * 2.5,
                ..default()
            }),
            yellow: materials.add(matte(Color::srgb(0.95, 0.8, 0.1))),
        }
    }
}

/// Builds the barrel under `root`: feet at the origin, upright.
pub(super) fn spawn(commands: &mut Commands, root: Entity, index: usize, a: &Assets) {
    let body = part(commands, root, &a.body, &a.red, Vec3::Y * 0.5);
    part(commands, root, &a.band, &a.yellow, Vec3::Y * 0.5);
    commands
        .entity(root)
        .insert((BarrelVisual(index), BarrelRig { body }));
}

/// Hides a dead barrel and flashes a hit one; the root's pose is fixed at spawn.
pub(super) fn pose_barrels(
    combat: Res<LevelCombat>,
    assets: Res<super::ActorAssets>,
    mut roots: Query<(&ActorVisual, &BarrelRig, &HitFlash, &mut Visibility)>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let a = &assets.barrel;
    for (g, rig, hit, mut vis) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let want = if actor.alive() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        vis.set_if_neq(want);
        let mat = if actor.alive() && hit.0 > 0.0 {
            &a.flash
        } else {
            &a.red
        };
        super::set_material(&mut mats, rig.body, mat);
    }
}
