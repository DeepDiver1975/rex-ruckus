//! Doors, lifts, switches and keycards: core `Mechanics` driven from FixedUpdate.

use crate::combat::{
    CombatSet, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
};
use crate::flow::{LevelDifficulty, LevelSource, PlayState};
use crate::level::CurrentMap;
use crate::player::{Inventory, Look, PendingInput, Player, PlayerBody, PlayerSimSet};
use crate::textures;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_core::hazard::HazardKind;
use rr_core::interact::use_target;
use rr_core::map::{Map, MaterialId, MoverKind, SectorId};
use rr_core::mechanics::{Mechanics, UseOutcome, UseTarget};
use rr_core::pickups::{Loadout, apply_pickup};
use rr_core::props::PropOutcome;
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

/// The hero's spoken line, on a subtitle line of its own so it never hides a [`HudMessage`].
#[derive(Resource, Default)]
pub struct HudSubtitle {
    pub text: String,
    pub remaining: f32,
}

impl HudSubtitle {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.remaining = MESSAGE_SECS;
    }
}

/// The live map and its mechanics for an authored map, in the start pose (doors closed).
pub fn fresh_level(mut map: Map) -> (CurrentMap, LevelMechanics) {
    let mech = Mechanics::new(&mut map);
    mark_crack_walls(&mut map);
    mark_hazard_floors(&mut map);
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
    let id = material_id(map, textures::CRACKED);
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

/// The id of material `name`, appended to `Map::materials` when missing.
fn material_id(map: &mut Map, name: &str) -> MaterialId {
    match map.materials.iter().position(|m| m == name) {
        Some(i) => i,
        None => {
            map.materials.push(name.to_string());
            map.materials.len() - 1
        }
    }
}

/// Points each hazard sector's floor at the `slime` or `electric` material (appended to
/// `Map::materials` when missing), like `mark_crack_walls` does for cracks.
pub fn mark_hazard_floors(map: &mut Map) {
    for s in 0..map.sectors.len() {
        let Some(h) = map.sectors[s].hazard else {
            continue;
        };
        let name = match h.kind {
            HazardKind::Slime => textures::SLIME,
            HazardKind::Electric => textures::ELECTRIC,
        };
        map.sectors[s].floor_mat = material_id(map, name);
    }
}

/// The level is won: freeze play on the completion screen.
pub fn finish_level(state: &mut PlayState, msg: &mut HudMessage) {
    *state = PlayState::Complete;
    msg.show("Level complete!");
}

/// Inserts the map and its mechanics in their start pose, and keeps the authored map in
/// [`LevelSource`] for restarts. The live map is the authored one as played on `difficulty`,
/// which is also stored as [`LevelDifficulty`].
pub fn insert_level(app: &mut App, map: Map, difficulty: Difficulty) {
    let (live, mech) = fresh_level(map.for_difficulty(difficulty));
    app.insert_resource(LevelSource(map))
        .insert_resource(LevelDifficulty(difficulty))
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
            .init_resource::<HudSubtitle>()
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
                        // Ungated: drains events from every mechanics call this tick, including
                        // `use_key` (before the player) and combat (cracks, channel fire).
                        forward_mech_events,
                    )
                        .chain()
                        .after(PlayerSimSet)
                        .after(CombatSet),
                ),
            );
    }
}

/// The HUD line for using a prop.
pub fn prop_message(o: PropOutcome) -> Option<String> {
    Some(match o {
        PropOutcome::Healed(n) => format!("+{n} health. Much better."),
        PropOutcome::Flushed => "Flush. Nothing like it.".into(),
        PropOutcome::Dispensed(0) => "Soda. Already topped up.".into(),
        PropOutcome::Dispensed(n) => format!("Soda! +{n} health"),
        PropOutcome::SoldOut => "Sold out.".into(),
        PropOutcome::Racked => "Racked 'em. No time for a game.".into(),
        PropOutcome::Busy => return None,
    })
}

/// What `use_key` reads per player; vitals are optional so headless tests may omit them.
type UseQuery = (
    &'static PlayerBody,
    &'static Look,
    &'static Inventory,
    &'static mut PendingInput,
    Option<&'static mut PlayerVitals>,
);

pub fn use_key(
    map: Res<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut prompt: ResMut<UsePrompt>,
    mut msg: ResMut<HudMessage>,
    mut state: ResMut<PlayState>,
    mut q: Query<UseQuery, With<Player>>,
) {
    for (body, look, inv, mut input, mut vitals) in &mut q {
        let target = use_target(&map.0, &mech.0, &body.0, look.angle);
        prompt.0 = target;
        if !std::mem::take(&mut input.use_pressed) {
            continue;
        }
        let Some(t) = target else { continue };
        if let UseTarget::Prop(i) = t {
            if let Some(v) = vitals.as_deref_mut()
                && let Some(text) = prop_message(mech.0.use_prop(i, &mut v.0))
            {
                msg.show(text);
            }
            continue;
        }
        match mech.0.activate(&map.0, t, inv.keys) {
            UseOutcome::Activated => {}
            UseOutcome::NeedKey(k) => msg.show(format!("You need the {} keycard", k.name())),
            UseOutcome::Exit => finish_level(&mut state, &mut msg),
        }
    }
}

/// Moves doors and lifts; the player and every living actor ride lifts and block doors.
#[allow(clippy::too_many_arguments)]
fn tick_movers(
    time: Res<Time<Fixed>>,
    mut map: ResMut<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut combat: ResMut<LevelCombat>,
    mut dirty: ResMut<DirtySectors>,
    mut state: ResMut<PlayState>,
    mut msg: ResMut<HudMessage>,
    mut q: Query<&mut PlayerBody>,
) {
    // Bodies slice: players first, then the living actors.
    let mut bodies: Vec<_> = q.iter().map(|b| b.0).collect();
    let players = bodies.len();
    let (idx, actors) = combat.0.living_bodies();
    bodies.extend(actors);
    // Entering a trigger sector fires its action (an Exit ends the level).
    for b in &bodies[..players] {
        if mech.0.enter(&map.0, b.sector) {
            finish_level(&mut state, &mut msg);
        }
    }
    let changed = mech
        .0
        .tick(&mut map.0, &mut bodies, time.timestep().as_secs_f32());
    for (mut b, moved) in q.iter_mut().zip(&bodies) {
        b.0 = *moved;
    }
    combat.0.write_back(&idx, &bodies[players..]);
    dirty.0.extend(changed);
}

/// Moves the events core `Mechanics` collected this tick into [`FxQueue`] for the frame loop.
fn forward_mech_events(mut mech: ResMut<LevelMechanics>, mut fx: ResMut<FxQueue>) {
    fx.mech.extend(mech.0.drain_events());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_messages() {
        assert_eq!(
            prop_message(PropOutcome::Healed(10)).as_deref(),
            Some("+10 health. Much better.")
        );
        assert_eq!(
            prop_message(PropOutcome::SoldOut).as_deref(),
            Some("Sold out.")
        );
        assert_eq!(prop_message(PropOutcome::Busy), None);
    }
}
