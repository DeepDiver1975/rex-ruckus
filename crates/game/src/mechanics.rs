//! Doors, lifts, switches and keycards: core `Mechanics` driven from FixedUpdate.

use crate::level::CurrentMap;
use crate::player::{Inventory, Look, PendingInput, Player, PlayerBody, PlayerSimSet};
use bevy::prelude::*;
use rr_core::interact::use_target;
use rr_core::map::{ItemKind, Map, SectorId};
use rr_core::mechanics::{Mechanics, UseOutcome, UseTarget};
use std::collections::BTreeSet;

/// How long a HUD message stays up, in seconds.
pub const MESSAGE_SECS: f32 = 2.5;

#[derive(Resource)]
pub struct LevelMechanics(pub Mechanics);

/// Sectors whose heights changed since the renderer last rebuilt them.
#[derive(Resource, Default)]
pub struct DirtySectors(pub BTreeSet<SectorId>);

/// What the use key points at right now (for the HUD prompt).
#[derive(Resource, Default)]
pub struct UsePrompt(pub Option<UseTarget>);

#[derive(Resource, Default)]
pub struct HudMessage {
    pub text: String,
    pub remaining: f32,
}

impl HudMessage {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.remaining = MESSAGE_SECS;
    }
}

#[derive(Resource, Default)]
pub struct LevelComplete(pub bool);

/// Inserts the map and its mechanics; this puts the level into its start pose (doors closed).
pub fn insert_level(app: &mut App, mut map: Map) {
    let mech = Mechanics::new(&mut map);
    app.insert_resource(CurrentMap(map))
        .insert_resource(LevelMechanics(mech));
}

/// Simulation only: safe to run headless. Needs `insert_level` and `PlayerSimPlugin`.
pub struct MechanicsSimPlugin;

impl Plugin for MechanicsSimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirtySectors>()
            .init_resource::<UsePrompt>()
            .init_resource::<HudMessage>()
            .init_resource::<LevelComplete>()
            .add_systems(
                FixedUpdate,
                (
                    use_key.before(PlayerSimSet),
                    (tick_movers, pickup_items).chain().after(PlayerSimSet),
                ),
            );
    }
}

fn use_key(
    map: Res<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut prompt: ResMut<UsePrompt>,
    mut msg: ResMut<HudMessage>,
    mut done: ResMut<LevelComplete>,
    mut q: Query<(&PlayerBody, &Look, &Inventory, &mut PendingInput), With<Player>>,
) {
    for (body, look, inv, mut input) in &mut q {
        let target = use_target(&map.0, &mech.0, &body.0, look.angle);
        prompt.0 = target;
        if !std::mem::take(&mut input.use_pressed) {
            continue;
        }
        let Some(t) = target else { continue };
        match mech.0.activate(&map.0, t, inv.keys) {
            UseOutcome::Activated => {}
            UseOutcome::NeedKey(k) => msg.show(format!("You need the {} keycard", k.name())),
            UseOutcome::Exit => {
                done.0 = true;
                msg.show("Level complete!");
            }
        }
    }
}

fn tick_movers(
    time: Res<Time<Fixed>>,
    mut map: ResMut<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut dirty: ResMut<DirtySectors>,
    mut q: Query<&mut PlayerBody>,
) {
    let mut bodies: Vec<_> = q.iter().map(|b| b.0).collect();
    let changed = mech
        .0
        .tick(&mut map.0, &mut bodies, time.timestep().as_secs_f32());
    for (mut b, moved) in q.iter_mut().zip(bodies) {
        b.0 = moved;
    }
    dirty.0.extend(changed);
}

fn pickup_items(
    map: Res<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut msg: ResMut<HudMessage>,
    mut q: Query<(&PlayerBody, &mut Inventory)>,
) {
    for (body, mut inv) in &mut q {
        // Temporary: non-key items are left in the world until `apply_pickup` is wired in.
        for i in mech
            .0
            .pickup(&map.0, &body.0, |k| matches!(k, ItemKind::Key(_)))
        {
            if let ItemKind::Key(k) = map.0.items[i].kind {
                inv.keys.insert(k);
                msg.show(format!("Picked up the {} keycard", k.name()));
            }
        }
    }
}
