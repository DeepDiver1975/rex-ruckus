//! Doors, lifts, switches and keycards: core `Mechanics` driven from FixedUpdate.

use crate::combat::{
    CombatSet, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
    clear_fx,
};
use crate::flow::{LevelDifficulty, LevelSource, PlayState, sim_ticks};
use crate::level::CurrentMap;
use crate::player::{
    Inventory, LocalId, LocalPlayer, Look, PendingInput, Player, PlayerBody, PlayerSimSet,
};
use crate::textures;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_core::hazard::HazardKind;
use rr_core::interact::use_target;
use rr_core::map::{Map, MaterialId, MoverKind, SectorId};
use rr_core::mechanics::{Mechanics, UseOutcome, UseTarget};
use rr_core::pickups::{Loadout, apply_pickup};
use rr_core::players::PlayerId;
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

/// HUD lines the simulation raised this tick, each with the player it is for: pickups, props,
/// a missing keycard, a jetpack or night vision running dry. [`show_local_lines`] puts the local
/// player's on the [`HudMessage`]; a host relays its seats' lines to them; the frame drops the
/// rest ([`drop_player_lines`]).
#[derive(Resource, Default, Debug)]
pub struct PlayerLines(pub Vec<(PlayerId, String)>);

impl PlayerLines {
    pub fn push(&mut self, player: PlayerId, text: impl Into<String>) {
        self.0.push((player, text.into()));
    }
}

/// Shows the local player's [`PlayerLines`] on the [`HudMessage`] (the last one wins) and takes
/// them out, at the end of the tick that raised them, so a line shows exactly when it did
/// before co-op. On the tick the level is won they are dropped: [`finish_level`]'s
/// "Level complete!" stays.
pub fn show_local_lines(
    local: LocalId,
    state: Res<PlayState>,
    mut lines: ResMut<PlayerLines>,
    mut msg: ResMut<HudMessage>,
) {
    let Some(id) = local.get() else { return };
    let won = *state == PlayState::Complete;
    lines.0.retain(|(p, text)| {
        if *p != id {
            return true;
        }
        if !won {
            msg.show(text.clone());
        }
        false
    });
}

/// Drops the lines no one took this frame (a partner's, with no host to relay them).
pub fn drop_player_lines(mut lines: ResMut<PlayerLines>) {
    lines.0.clear();
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

/// Points each hazard sector's floor at the `slime`, `electric` or `magma` material (appended to
/// `Map::materials` when missing), like `mark_crack_walls` does for cracks.
pub fn mark_hazard_floors(map: &mut Map) {
    for s in 0..map.sectors.len() {
        let Some(h) = map.sectors[s].hazard else {
            continue;
        };
        let name = match h.kind {
            HazardKind::Slime => textures::SLIME,
            HazardKind::Electric => textures::ELECTRIC,
            HazardKind::Burn => textures::MAGMA,
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
            .init_resource::<PlayerLines>()
            .init_resource::<PlayState>()
            .add_systems(Update, drop_player_lines.before(clear_fx))
            .add_systems(
                FixedUpdate,
                (
                    use_key.before(PlayerSimSet).run_if(sim_ticks),
                    // Gated one by one (not as a group) so a state change earlier in the tick
                    // stops every later system of the same tick.
                    (
                        tick_movers.run_if(sim_ticks),
                        pickup_items.run_if(sim_ticks),
                        // Ungated: drains events from every mechanics call this tick, including
                        // `use_key` (before the player) and combat (cracks, channel fire).
                        forward_mech_events,
                        show_local_lines,
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
        PropOutcome::Played => "GAME OVER. NEW HIGH SCORE: REX".into(),
        PropOutcome::Stinger => "The jukebox kicks out a riff.".into(),
        PropOutcome::Tip(0) => "A voice rasps: \"You found them all.\" Click.".into(),
        PropOutcome::Tip(1) => "A voice rasps: \"1 secret still out there.\"".into(),
        PropOutcome::Tip(n) => format!("A voice rasps: \"{n} secrets still out there.\""),
        PropOutcome::DialTone => "Just a dial tone.".into(),
        PropOutcome::Busy => return None,
    })
}

/// What `use_key` reads per player; vitals are optional so headless tests may omit them.
type UseQuery = (
    &'static Player,
    Has<LocalPlayer>,
    &'static PlayerBody,
    &'static Look,
    &'static Inventory,
    &'static mut PendingInput,
    Option<&'static mut PlayerVitals>,
);

#[allow(clippy::too_many_arguments)]
pub fn use_key(
    map: Res<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    combat: Option<Res<LevelCombat>>,
    mut prompt: ResMut<UsePrompt>,
    mut msg: ResMut<HudMessage>,
    mut lines: ResMut<PlayerLines>,
    mut state: ResMut<PlayState>,
    mut q: Query<UseQuery, With<Player>>,
) {
    // In id order, so two players on one switch or door settle it the same way on every machine.
    let mut players: Vec<_> = q.iter_mut().collect();
    players.sort_by_key(|(p, ..)| p.id);
    for (player, local, body, look, inv, mut input, mut vitals) in players {
        let target = use_target(&map.0, &mech.0, &body.0, look.angle);
        if local {
            prompt.0 = target;
        }
        // The dead (a co-op partner still plays on) use nothing.
        let alive = vitals.as_ref().is_none_or(|v| v.0.health.alive());
        if !std::mem::take(&mut input.use_pressed) || !alive {
            continue;
        }
        let Some(t) = target else { continue };
        if let UseTarget::Prop(i) = t {
            // What a payphone tip reports.
            let secrets_left = combat.as_ref().map_or(0, |c| c.0.destruct.secrets_left());
            // A partner's prop and key messages show on their HUD, not this one.
            if let Some(v) = vitals.as_deref_mut()
                && let Some(text) = prop_message(mech.0.use_prop(i, &mut v.0, secrets_left))
            {
                lines.push(player.id, text);
            }
            continue;
        }
        match mech.0.activate(&map.0, player.id, t, inv.keys) {
            UseOutcome::Activated => {}
            UseOutcome::NeedKey(k) => {
                lines.push(player.id, format!("You need the {} keycard", k.name()))
            }
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
    mut q: Query<(&Player, &mut PlayerBody)>,
) {
    // Bodies slice: players first in id order (`bodies[k]` is `PlayerId(k)`), then the living
    // actors.
    let mut sorted: Vec<_> = q.iter_mut().collect();
    sorted.sort_by_key(|(p, _)| p.id);
    let mut bodies: Vec<_> = sorted.iter().map(|(_, b)| b.0).collect();
    let players = bodies.len();
    let (idx, actors) = combat.0.living_bodies();
    bodies.extend(actors);
    // Entering a trigger sector fires its action (an Exit ends the level).
    for (k, b) in bodies[..players].iter().enumerate() {
        if mech.0.enter(&map.0, PlayerId(k as u8), b.sector) {
            finish_level(&mut state, &mut msg);
        }
    }
    let changed = mech
        .0
        .tick(&mut map.0, &mut bodies, time.timestep().as_secs_f32());
    for ((_, b), moved) in sorted.iter_mut().zip(&bodies) {
        b.0 = *moved;
    }
    combat.0.write_back(&idx, &bodies[players..]);
    dirty.0.extend(changed);
}

/// Moves the events core `Mechanics` collected this tick into [`FxQueue`] for the frame loop.
pub(crate) fn forward_mech_events(mut mech: ResMut<LevelMechanics>, mut fx: ResMut<FxQueue>) {
    fx.mech.extend(mech.0.drain_events());
}

/// Walking over items: `apply_pickup` decides, and a refused item stays in the world.
/// The dead pick nothing up; each pickup's text is a [`PlayerLines`] line for its player.
#[allow(clippy::type_complexity)]
pub fn pickup_items(
    map: Res<CurrentMap>,
    defs: Res<GameDefs>,
    mut mech: ResMut<LevelMechanics>,
    mut lines: ResMut<PlayerLines>,
    mut q: Query<(
        &Player,
        &PlayerBody,
        &mut Inventory,
        &mut PlayerVitals,
        &mut PlayerArsenal,
        &mut PlayerInventory,
    )>,
) {
    // In id order, so two players on one item settle it the same way on every machine.
    let mut players: Vec<_> = q.iter_mut().collect();
    players.sort_by_key(|(p, ..)| p.id);
    for (player, body, mut inv, mut health, mut arsenal, mut carried) in players {
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
                .map(|text| lines.push(player.id, text))
                .is_some()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_shown(state: PlayState) -> String {
        let mut world = World::new();
        world.spawn((Player { id: PlayerId(0) }, LocalPlayer));
        world.insert_resource(state);
        let mut msg = HudMessage::default();
        let mut lines = PlayerLines::default();
        if state == PlayState::Complete {
            let mut s = PlayState::Playing;
            finish_level(&mut s, &mut msg);
        }
        lines.push(PlayerId(0), "Picked up a medkit");
        world.insert_resource(msg);
        world.insert_resource(lines);
        world.run_system_cached(show_local_lines).unwrap();
        assert!(world.resource::<PlayerLines>().0.is_empty(), "taken");
        world.resource::<HudMessage>().text.clone()
    }

    #[test]
    fn level_complete_beats_a_line_of_the_same_tick() {
        assert_eq!(lines_shown(PlayState::Playing), "Picked up a medkit");
        assert_eq!(lines_shown(PlayState::Complete), "Level complete!");
    }

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
        assert_eq!(
            prop_message(PropOutcome::Tip(2)).as_deref(),
            Some("A voice rasps: \"2 secrets still out there.\"")
        );
        assert!(
            prop_message(PropOutcome::Tip(1))
                .unwrap()
                .contains("1 secret still")
        );
        for o in [
            PropOutcome::Played,
            PropOutcome::Stinger,
            PropOutcome::Tip(0),
            PropOutcome::DialTone,
        ] {
            let text = prop_message(o).unwrap();
            assert!(!text.is_empty() && text.len() <= 48, "{text}");
        }
    }
}
