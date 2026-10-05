//! Weapons, enemies and bolts: core `Arsenal` and `Combat` driven from FixedUpdate. Core events
//! are collected in [`FxQueue`] for frame-loop systems (sound, sparks, HUD) to drain.

use crate::flow::{PlayState, SpawnLevel};
use crate::level::CurrentMap;
use crate::paths::assets_dir;
use crate::player::{
    EYE_BELOW_TOP, Look, PendingInput, Player, PlayerBody, PlayerSimSet, spawn_player,
};
use bevy::prelude::*;
use rr_core::collide::Body;
use rr_core::combat::{Combat, CombatEvent, PlayerTarget, level_seed};
use rr_core::defs::Defs;
use rr_core::health::{Health, PLAYER_MAX_HEALTH};
use rr_core::map::Map;
use rr_core::rng::Rng;
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

#[derive(Component)]
pub struct PlayerHealth(pub Health);

#[derive(Component)]
pub struct PlayerArsenal(pub Arsenal);

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

/// A fresh player loadout: full health and the starting weapons.
pub fn player_loadout(defs: &Defs) -> (PlayerHealth, PlayerArsenal) {
    (
        PlayerHealth(Health::new(PLAYER_MAX_HEALTH)),
        PlayerArsenal(Arsenal::new(defs)),
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

/// Simulation only: safe to run headless. Needs `insert_level`, `insert_defs` and
/// `PlayerSimPlugin`.
pub struct CombatSimPlugin;

impl Plugin for CombatSimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxQueue>()
            .init_resource::<PlayState>()
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
            );
    }
}

fn spawn_combat(
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

#[allow(clippy::type_complexity)]
fn player_weapons(
    time: Res<Time<Fixed>>,
    map: Res<CurrentMap>,
    defs: Res<GameDefs>,
    mut combat: ResMut<LevelCombat>,
    mut rng: ResMut<PlayRng>,
    mut fx: ResMut<FxQueue>,
    mut q: Query<
        (
            &PlayerBody,
            &Look,
            &PlayerHealth,
            &mut PlayerArsenal,
            &mut PendingInput,
        ),
        With<Player>,
    >,
) {
    let dt = time.timestep().as_secs_f32();
    for (body, look, health, mut arsenal, mut input) in &mut q {
        // Consume the latches every tick so presses never pile up.
        let input = WeaponInput {
            // A tap between ticks fires at least once even though the button is up again.
            fire: input.fire | std::mem::take(&mut input.fire_pressed),
            reload: std::mem::take(&mut input.reload),
            kick: std::mem::take(&mut input.kick),
            select: input.select.take(),
            cycle: std::mem::take(&mut input.cycle),
        };
        if !health.0.alive() {
            continue;
        }
        let aim = aim_dir(look.angle, look.pitch);
        let events = arsenal.0.tick(&defs.0, &input, aim, &mut rng.0, dt);
        let eye = eye_of(&body.0);
        for ev in &events {
            if matches!(ev, WeaponEvent::Fire { .. } | WeaponEvent::Kick { .. }) {
                let out = combat
                    .0
                    .player_attack(&map.0, &defs.0, eye, body.0.sector, ev);
                fx.combat.extend(out);
            }
        }
        fx.weapon.extend(events);
    }
}

fn combat_tick(
    time: Res<Time<Fixed>>,
    map: Res<CurrentMap>,
    defs: Res<GameDefs>,
    mut combat: ResMut<LevelCombat>,
    mut fx: ResMut<FxQueue>,
    mut q: Query<(&mut PlayerBody, &mut PlayerHealth), With<Player>>,
) {
    let dt = time.timestep().as_secs_f32();
    for (mut body, mut health) in &mut q {
        let eye = eye_of(&body.0);
        let mut target = PlayerTarget {
            body: &mut body.0,
            health: &mut health.0,
            eye,
        };
        let out = combat.0.tick(&map.0, &defs.0, &mut target, dt);
        fx.combat.extend(out);
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
