//! Doors, lifts, switches and keycards: core `Mechanics` driven from FixedUpdate.

use crate::combat::{
    CombatSet, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
};
use crate::flow::{LevelSource, PlayState};
use crate::level::CurrentMap;
use crate::player::{Inventory, Look, PendingInput, Player, PlayerBody, PlayerSimSet};
use crate::textures;
use bevy::prelude::*;
use rr_core::interact::use_target;
use rr_core::map::{Map, MoverKind, SectorId};
use rr_core::mechanics::{Mechanics, UseOutcome, UseTarget};
use rr_core::pickups::{Loadout, apply_pickup};
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

/// The live map and its mechanics for an authored map, in the start pose (doors closed).
pub fn fresh_level(mut map: Map) -> (CurrentMap, LevelMechanics) {
    let mech = Mechanics::new(&mut map);
    mark_crack_walls(&mut map);
    (CurrentMap(map), LevelMechanics(mech))
}

/// Gives crack walls the cracked-concrete look: the step faces neighbours draw into a crack
/// sector use its `face_mat`, and its own side walls use `wall_mat`; both are pointed at the
/// `cracked` material, which is appended to `Map::materials` when missing. Done on the live map
/// at load (the core knows nothing of textures), so restarts redo it from the authored map.
pub fn mark_crack_walls(map: &mut Map) {
    let is_crack = |s: &rr_core::map::Sector| s.mover.is_some_and(|m| m.kind == MoverKind::Crack);
    if !map.sectors.iter().any(is_crack) {
        return;
    }
    let id = match map.materials.iter().position(|m| m == textures::CRACKED) {
        Some(i) => i,
        None => {
            map.materials.push(textures::CRACKED.to_string());
            map.materials.len() - 1
        }
    };
    for s in 0..map.sectors.len() {
        if !is_crack(&map.sectors[s]) {
            continue;
        }
        map.sectors[s].face_mat = Some(id);
        map.sectors[s].wall_mat = id;
        let walls: Vec<usize> = map.sectors[s].walls().collect();
        for w in walls {
            map.walls[w].material = id;
        }
    }
}

/// Inserts the map and its mechanics in their start pose, and keeps the authored map in
/// [`LevelSource`] for restarts.
pub fn insert_level(app: &mut App, map: Map) {
    let (live, mech) = fresh_level(map.clone());
    app.insert_resource(LevelSource(map))
        .insert_resource(live)
        .insert_resource(mech);
}

/// Simulation only: safe to run headless. Needs `insert_level`, `PlayerSimPlugin` and
/// `CombatSimPlugin` (movers carry actors; pickups feed the player's health and arsenal).
pub struct MechanicsSimPlugin;

impl Plugin for MechanicsSimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirtySectors>()
            .init_resource::<UsePrompt>()
            .init_resource::<HudMessage>()
            .init_resource::<PlayState>()
            .add_systems(
                FixedUpdate,
                (
                    use_key
                        .before(PlayerSimSet)
                        .run_if(resource_equals(PlayState::Playing)),
                    // Gated one by one (not as a group) so a state change earlier in the tick
                    // stops every later system of the same tick.
                    (
                        tick_movers.run_if(resource_equals(PlayState::Playing)),
                        pickup_items.run_if(resource_equals(PlayState::Playing)),
                    )
                        .chain()
                        .after(PlayerSimSet)
                        .after(CombatSet),
                ),
            );
    }
}

fn use_key(
    map: Res<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut prompt: ResMut<UsePrompt>,
    mut msg: ResMut<HudMessage>,
    mut state: ResMut<PlayState>,
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
                *state = PlayState::Complete;
                msg.show("Level complete!");
            }
        }
    }
}

/// Moves doors and lifts; the player and every living actor ride lifts and block doors.
fn tick_movers(
    time: Res<Time<Fixed>>,
    mut map: ResMut<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut combat: ResMut<LevelCombat>,
    mut dirty: ResMut<DirtySectors>,
    mut q: Query<&mut PlayerBody>,
) {
    // Bodies slice: players first, then the living actors.
    let mut bodies: Vec<_> = q.iter().map(|b| b.0).collect();
    let players = bodies.len();
    let (idx, actors) = combat.0.living_bodies();
    bodies.extend(actors);
    let changed = mech
        .0
        .tick(&mut map.0, &mut bodies, time.timestep().as_secs_f32());
    for (mut b, moved) in q.iter_mut().zip(&bodies) {
        b.0 = *moved;
    }
    combat.0.write_back(&idx, &bodies[players..]);
    dirty.0.extend(changed);
}

/// Walking over items: `apply_pickup` decides, and a refused item stays in the world.
/// The dead pick nothing up.
pub fn pickup_items(
    map: Res<CurrentMap>,
    defs: Res<GameDefs>,
    mut mech: ResMut<LevelMechanics>,
    mut msg: ResMut<HudMessage>,
    mut q: Query<(
        &PlayerBody,
        &mut Inventory,
        &mut PlayerVitals,
        &mut PlayerArsenal,
        &mut PlayerInventory,
    )>,
) {
    for (body, mut inv, mut health, mut arsenal, mut carried) in &mut q {
        if !health.0.health.alive() {
            continue;
        }
        let mut loadout = Loadout {
            vitals: &mut health.0,
            arsenal: &mut arsenal.0,
            keys: &mut inv.keys,
            inventory: &mut carried.0,
        };
        mech.0.pickup(&map.0, &body.0, |kind| {
            apply_pickup(&defs.0, kind, &mut loadout)
                .map(|text| msg.show(text))
                .is_some()
        });
    }
}
