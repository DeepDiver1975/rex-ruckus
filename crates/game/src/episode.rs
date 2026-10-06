//! The episode: an ordered list of levels played on one difficulty, with the loadout carried from
//! level to level and the stats added up for the episode-end screen.

use crate::combat::{
    CombatSet, FxQueue, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
};
use crate::flow::{LevelDifficulty, PlayState, SpawnLevel, load_level};
use crate::level::load_map;
use crate::paths::assets_dir;
use crate::player::Player;
use bevy::prelude::*;
use rr_core::carry::CarryOver;
use rr_core::difficulty::Difficulty;
use rr_core::map::Map;
use rr_core::stats::LevelStats;
use serde::Deserialize;

/// `assets/episode.ron`: the levels in play order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EpisodeDef {
    pub name: String,
    /// File names inside `assets/levels/`.
    pub levels: Vec<String>,
}

impl EpisodeDef {
    pub fn from_ron(src: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(src)
    }
}

/// Reads `assets/episode.ron`, panicking with the path on failure.
pub fn load_episode() -> EpisodeDef {
    let path = assets_dir().join("episode.ron");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    EpisodeDef::from_ron(&src).unwrap_or_else(|e| panic!("invalid {}: {e}", path.display()))
}

/// Progress through the episode. Absent for direct-level runs.
#[derive(Resource, Debug)]
pub struct Episode {
    pub def: EpisodeDef,
    /// Index into the level list of the level being played.
    pub index: usize,
    /// Loadout the player entered the current level with; `None` = the starting loadout.
    pub entry: Option<CarryOver>,
    /// Stats of the finished levels.
    pub totals: LevelStats,
    /// Pre-built maps used instead of loading `def.levels` (test/demo hook).
    pub maps: Option<Vec<Map>>,
}

impl Episode {
    pub fn new(def: EpisodeDef) -> Self {
        Episode {
            def,
            index: 0,
            entry: None,
            totals: LevelStats::default(),
            maps: None,
        }
    }

    /// Number of levels: the test maps when set, else the episode file's list.
    pub fn len(&self) -> usize {
        self.maps.as_ref().map_or(self.def.levels.len(), Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The authored map of level `i`.
    pub fn map(&self, i: usize) -> Map {
        match &self.maps {
            Some(maps) => maps[i].clone(),
            None => load_map(&self.def.levels[i]),
        }
    }
}

/// Tallies for the level being played.
#[derive(Resource, Debug, Default)]
pub struct Stats(pub LevelStats);

/// Starts the episode over on `d`: level 0 with the starting loadout and zeroed totals.
pub fn start_episode(world: &mut World, d: Difficulty) {
    world.resource_mut::<LevelDifficulty>().0 = d;
    let map = {
        let mut ep = world.resource_mut::<Episode>();
        ep.index = 0;
        ep.entry = None;
        ep.totals = LevelStats::default();
        ep.map(0)
    };
    load_level(world, map);
}

/// After a completed level: carry the loadout on and load the next level, or end the episode.
pub fn advance(world: &mut World) {
    let carry = world
        .query_filtered::<(&PlayerVitals, &PlayerArsenal, &PlayerInventory), With<Player>>()
        .iter(world)
        .next()
        .map(|(v, a, i)| CarryOver::take(&v.0, &a.0, &i.0));
    let stats = world.resource::<Stats>().0;
    let next = {
        let mut ep = world.resource_mut::<Episode>();
        ep.totals.add(&stats);
        ep.entry = carry;
        ep.index += 1;
        (ep.index < ep.len()).then(|| ep.map(ep.index))
    };
    match next {
        Some(map) => load_level(world, map),
        None => *world.resource_mut::<PlayState>() = PlayState::EpisodeEnd,
    }
}

/// Level stats: reset at every spawn, fed by the fixed tick.
pub struct EpisodePlugin;

impl Plugin for EpisodePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stats>()
            .add_systems(SpawnLevel, reset_stats.after(crate::combat::spawn_combat))
            .add_systems(
                FixedUpdate,
                record_stats
                    .after(CombatSet)
                    .run_if(resource_equals(PlayState::Playing)),
            );
    }
}

fn reset_stats(mut stats: ResMut<Stats>, combat: Res<LevelCombat>) {
    stats.0 = LevelStats::new(&combat.0);
}

/// Counts the combat events queued since the last tick (several ticks can run per frame, and the
/// queue is emptied only once per frame) and the tick itself.
fn record_stats(mut stats: ResMut<Stats>, combat: Res<LevelCombat>, mut fx: ResMut<FxQueue>) {
    let seen = fx.stats_seen.min(fx.combat.len());
    stats.0.record(&combat.0, &fx.combat[seen..]);
    stats.0.tick();
    fx.stats_seen = fx.combat.len();
}
