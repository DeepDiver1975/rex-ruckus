//! The explosive barrel on its glTF model. It never animates and simply disappears when it
//! dies; its explosion effect covers the moment. Its hit flash is the shared look swap.

use super::ActorVisual;
use crate::combat::LevelCombat;
use bevy::prelude::*;

/// Marks the root of a barrel's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct BarrelVisual(pub usize);

/// Hides a dead barrel; the root's pose is fixed at spawn.
pub(super) fn hide_dead_barrels(
    combat: Res<LevelCombat>,
    mut roots: Query<(&ActorVisual, &mut Visibility), With<BarrelVisual>>,
) {
    for (g, mut vis) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let want = if actor.alive() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        vis.set_if_neq(want);
    }
}
