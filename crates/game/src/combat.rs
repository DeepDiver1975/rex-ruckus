//! Weapons, enemies and bolts: core `Arsenal` and `Combat` driven from FixedUpdate. Core events
//! are collected in [`FxQueue`]; frame-loop readers (sparks, HUD, …) run in [`FxReaders`] and
//! [`clear_fx`] empties the queue after them.

use crate::flow::{PlayState, SpawnLevel};
use crate::level::CurrentMap;
use crate::mechanics::{DirtySectors, LevelMechanics};
use crate::paths::assets_dir;
use crate::player::{
    EYE_BELOW_TOP, Look, PendingInput, Player, PlayerBody, PlayerSimSet, spawn_player,
};
use bevy::prelude::*;
use rr_core::collide::Body;
use rr_core::combat::{Combat, CombatEvent, PlayerTarget, level_seed};
use rr_core::defs::Defs;
use rr_core::inventory::Inventory as CoreInventory;
use rr_core::map::Map;
use rr_core::rng::Rng;
use rr_core::vitals::Vitals;
use rr_core::weapons::{Arsenal, WeaponEvent, WeaponInput};

/// Mixed into the level seed so the player's weapon spread does not share a stream with combat.
const PLAY_RNG_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// Weapon and enemy tuning.
#[derive(Resource)]
pub struct GameDefs(pub Defs);

/// Enemies and bolts of the current level.
#[derive(Resource)]
pub struct LevelCombat(pub Combat);

/// Core events produced by the fixed tick, waiting for frame-loop systems to drain them.
/// A plain resource (not Bevy messages) so hand-driven `FixedUpdate` runs never lose events.
#[derive(Resource, Default)]
pub struct FxQueue {
    pub combat: Vec<CombatEvent>,
    pub weapon: Vec<WeaponEvent>,
}

/// Randomness for the player's weapons (shot spread), seeded from the level.
#[derive(Resource)]
pub struct PlayRng(pub Rng);

/// The player's health and armour.
#[derive(Component)]
pub struct PlayerVitals(pub Vitals);

#[derive(Component)]
pub struct PlayerArsenal(pub Arsenal);

/// The player's carried medkit charge, jetpack fuel and night-vision battery (empty at spawn).
#[derive(Component, Default)]
pub struct PlayerInventory(pub CoreInventory);

/// Every `Update` system that reads [`FxQueue`]. Readers only read; [`clear_fx`] runs after
/// the set and is the only system that empties the queue.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FxReaders;

/// Combat's per-tick step; `tick_movers` and pickups run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

/// Loads `assets/defs/{weapons,enemies}.ron`, panicking with the path on failure.
pub fn load_defs() -> Defs {
    let dir = assets_dir().join("defs");
    let read = |f: &str| {
        let path = dir.join(f);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read defs {}: {e}", path.display()))
    };
    Defs::from_ron(&read("weapons.ron"), &read("enemies.ron"))
        .unwrap_or_else(|e| panic!("invalid defs in {}: {e:?}", dir.display()))
}

pub fn insert_defs(app: &mut App, defs: Defs) {
    app.insert_resource(GameDefs(defs));
}

/// The level's combat state and the player's weapon RNG, both seeded from the map name.
pub fn level_combat(map: &Map, defs: &Defs) -> (LevelCombat, PlayRng) {
    let seed = level_seed(&map.name);
    (
        LevelCombat(Combat::spawn(map, defs, seed)),
        PlayRng(Rng::new(seed ^ PLAY_RNG_SALT)),
    )
}

/// A fresh player loadout: full health, no armour and the starting weapons.
pub fn player_loadout(defs: &Defs) -> (PlayerVitals, PlayerArsenal, PlayerInventory) {
    (
        PlayerVitals(Vitals::new()),
        PlayerArsenal(Arsenal::new(defs)),
        PlayerInventory::default(),
    )
}

/// The player's eye point in core coordinates.
pub fn eye_of(body: &Body) -> Vec3 {
    body.pos + Vec3::Z * (body.height - EYE_BELOW_TOP)
}

/// Unit view direction for a core heading `angle` and an up-positive `pitch`.
pub fn aim_dir(angle: f32, pitch: f32) -> Vec3 {
    let (sp, cp) = pitch.sin_cos();
    let (sa, ca) = angle.sin_cos();
    Vec3::new(cp * ca, cp * sa, sp)
}

/// Simulation only: safe to run headless. Needs `insert_level` (map and mechanics; shattered
/// glass changes the map and lands in `DirtySectors`), `insert_defs` and `PlayerSimPlugin`.
pub struct CombatSimPlugin;

impl Plugin for CombatSimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxQueue>()
            .init_resource::<PlayState>()
            .init_resource::<DirtySectors>()
            .add_systems(SpawnLevel, spawn_combat.after(spawn_player))
            .add_systems(
                FixedUpdate,
                (
                    player_weapons.run_if(resource_equals(PlayState::Playing)),
                    combat_tick
                        .in_set(CombatSet)
                        .run_if(resource_equals(PlayState::Playing)),
                )
                    .chain()
                    .after(PlayerSimSet),
            )
            .add_systems(Update, clear_fx.after(FxReaders));
    }
}

/// Empties [`FxQueue`] once per frame, after every [`FxReaders`] system has seen this frame's
/// events. Registered by the sim plugin so the queue cannot grow without a renderer either.
pub fn clear_fx(mut fx: ResMut<FxQueue>) {
    fx.combat.clear();
    fx.weapon.clear();
}

/// Inserts the level's [`LevelCombat`] and [`PlayRng`] and the player's loadout.
pub fn spawn_combat(
    mut commands: Commands,
    map: Res<CurrentMap>,
    defs: Res<GameDefs>,
    players: Query<Entity, With<Player>>,
) {
    let (combat, rng) = level_combat(&map.0, &defs.0);
    commands.insert_resource(combat);
    commands.insert_resource(rng);
    for e in &players {
        commands.entity(e).insert(player_loadout(&defs.0));
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn player_weapons(
    time: Res<Time<Fixed>>,
    mut map: ResMut<CurrentMap>,
    mut dirty: ResMut<DirtySectors>,
    defs: Res<GameDefs>,
    mut combat: ResMut<LevelCombat>,
    mut rng: ResMut<PlayRng>,
    mut fx: ResMut<FxQueue>,
    mut q: Query<
        (
            &PlayerBody,
            &Look,
            &PlayerVitals,
            &mut PlayerArsenal,
            &mut PendingInput,
        ),
        With<Player>,
    >,
) {
    let dt = time.timestep().as_secs_f32();
    for (body, look, vitals, mut arsenal, mut input) in &mut q {
        // Consume the latches every tick so presses never pile up.
        // Inventory taps are consumed here and ignored until Task 21 wires them.
        let _ = (
            std::mem::take(&mut input.use_medkit),
            std::mem::take(&mut input.toggle_jetpack),
            std::mem::take(&mut input.toggle_nv),
        );
        let input = WeaponInput {
            // A tap between ticks fires at least once even though the button is up again.
            fire: input.fire | input.fire_pressed,
            fire_pressed: std::mem::take(&mut input.fire_pressed),
            reload: std::mem::take(&mut input.reload),
            kick: std::mem::take(&mut input.kick),
            select: input.select.take(),
            cycle: std::mem::take(&mut input.cycle),
        };
        if !vitals.0.health.alive() {
            continue;
        }
        let aim = aim_dir(look.angle, look.pitch);
        // Bombs still in the world keep the launcher armed for detonation.
        arsenal.0.live_bombs = combat.0.live_bombs();
        let events = arsenal.0.tick(&defs.0, &input, aim, &mut rng.0, dt);
        let eye = eye_of(&body.0);
        for ev in &events {
            if matches!(
                ev,
                WeaponEvent::Fire { .. }
                    | WeaponEvent::Kick { .. }
                    | WeaponEvent::Launch { .. }
                    | WeaponEvent::Detonate
            ) {
                let out = combat
                    .0
                    .player_attack(&mut map.0, &defs.0, eye, body.0.sector, ev);
                mark_dirty(&out, &mut dirty);
                fx.combat.extend(out);
            }
        }
        fx.weapon.extend(events);
    }
}

#[allow(clippy::too_many_arguments)]
fn combat_tick(
    time: Res<Time<Fixed>>,
    mut map: ResMut<CurrentMap>,
    mut mech: ResMut<LevelMechanics>,
    mut dirty: ResMut<DirtySectors>,
    defs: Res<GameDefs>,
    mut combat: ResMut<LevelCombat>,
    mut fx: ResMut<FxQueue>,
    mut q: Query<(&mut PlayerBody, &mut PlayerVitals), With<Player>>,
) {
    let dt = time.timestep().as_secs_f32();
    for (mut body, mut vitals) in &mut q {
        let eye = eye_of(&body.0);
        let mut target = PlayerTarget {
            body: &mut body.0,
            vitals: &mut vitals.0,
            eye,
        };
        let out = combat
            .0
            .tick(&mut map.0, &mut mech.0, &defs.0, &mut target, dt);
        mark_dirty(&out, &mut dirty);
        fx.combat.extend(out);
    }
}

/// Queues the sectors a combat change (shattered glass) left stale for re-meshing, the same
/// path movers use. The shard effect reads `GlassBroken` from [`FxQueue`] separately.
fn mark_dirty(events: &[CombatEvent], dirty: &mut DirtySectors) {
    for e in events {
        if let CombatEvent::GlassBroken { dirty: sectors, .. } = e {
            dirty.0.extend(sectors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aim_follows_heading_and_pitch_up_positive() {
        let east = aim_dir(0.0, 0.0);
        assert!((east - Vec3::X).length() < 1e-6);
        let north = aim_dir(std::f32::consts::FRAC_PI_2, 0.0);
        assert!((north - Vec3::Y).length() < 1e-6);
        let up = aim_dir(0.0, 0.5);
        assert!(up.z > 0.0 && (up.length() - 1.0).abs() < 1e-6);
    }
}
