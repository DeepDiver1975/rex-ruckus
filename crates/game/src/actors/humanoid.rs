//! The walking enemies on glTF models: the Grunt (pistol soldier), the Enforcer (armoured
//! trooper), the Slasher (melee alien) and the boss Warlord. Animation and looks are shared with
//! every enemy (see [`super::anim`]); this module places them and hands the Grunt its pistol.

use super::{ActorModel, ActorVisual, KeepWorldScale, place_walker};
use crate::combat::LevelCombat;
use crate::models::{ModelLibrary, ModelReady, ModelSlot, spawn_model_with};
use bevy::prelude::*;

/// Marks the root of a Grunt's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct GruntVisual(pub usize);

/// Marks the root of an Enforcer's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct EnforcerVisual(pub usize);

/// Marks the root of a Slasher's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct SlasherVisual(pub usize);

/// Marks the root of the boss's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct BossVisual(pub usize);

/// The attach scene (e.g. the Grunt's pistol) has been parented to its bone.
#[derive(Component)]
pub(super) struct Attached;

type Walker = Or<(
    With<GruntVisual>,
    With<EnforcerVisual>,
    With<SlasherVisual>,
    With<BossVisual>,
)>;

/// Places every walking enemy between its last two ticks, facing its heading.
pub(super) fn place_walkers(
    fixed: Res<Time<Fixed>>,
    combat: Res<LevelCombat>,
    mut roots: Query<(&ActorVisual, &mut Transform), Walker>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, mut t) in &mut roots {
        if let Some(actor) = combat.0.actors.get(g.0) {
            place_walker(&mut t, actor, alpha);
        }
    }
}

/// Parents an enemy's attach scene to its bone once the model is ready. The bone sits inside
/// the glTF's scaled armature, so the attachment keeps its placement's scale and offset in
/// world units ([`KeepWorldScale`]); its rotation stays relative to the bone.
pub(super) fn attach_scenes(
    mut commands: Commands,
    lib: Res<ModelLibrary>,
    roots: Query<(Entity, &ActorModel), Without<Attached>>,
    ready: Query<&ModelReady>,
) {
    for (root, model) in &roots {
        let Some(attach) = &lib.enemy(model.kind).attach else {
            commands.entity(root).insert(Attached);
            continue;
        };
        let Ok(ready) = ready.get(model.root) else {
            continue;
        };
        if let Some(&bone) = ready.nodes.get(&attach.bone) {
            let place = &attach.model.place;
            let e = spawn_model_with(
                &mut commands,
                bone,
                &attach.model.scene,
                place,
                ModelSlot::default(),
            );
            commands.entity(e).insert(KeepWorldScale {
                scale: place.scale,
                offset: Vec3::from(place.offset),
            });
        } else {
            warn!("{:?}: no bone named {:?}", model.kind, attach.bone);
        }
        commands.entity(root).insert(Attached);
    }
}
