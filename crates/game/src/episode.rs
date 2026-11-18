//! The episode: an ordered list of levels played on one difficulty, with the loadout carried from
//! level to level and the stats added up for the episode-end screen.

use crate::combat::{
    CombatSet, FxQueue, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
};
use crate::coop::{CoopPlugin, EntryLoadout};
use crate::flow::{LevelDifficulty, LevelFile, PlayState, SpawnLevel, load_level, sim_ticks};
use crate::level::load_map;
use crate::paths::assets_dir;
use crate::player::Player;
use bevy::prelude::*;
use rr_core::carry::CarryOver;
use rr_core::difficulty::Difficulty;
use rr_core::map::Map;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::stats::LevelStats;
use serde::Deserialize;

/// One episode of `assets/episode.ron`: its name and the levels in play order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeDef {
    pub name: String,
    /// File names inside `assets/levels/`. Empty: announced but not playable yet ("coming soon").
    pub levels: Vec<String>,
}

impl EpisodeDef {
    /// Whether the episode has levels to play.
    pub fn playable(&self) -> bool {
        !self.levels.is_empty()
    }
}

/// `assets/episode.ron`: the episodes in menu order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeList {
    pub episodes: Vec<EpisodeDef>,
}

impl EpisodeList {
    /// Parses the file; at least one episode must be playable.
    pub fn from_ron(src: &str) -> Result<Self, String> {
        let list: EpisodeList = ron::from_str(src).map_err(|e| e.to_string())?;
        if !list.episodes.iter().any(EpisodeDef::playable) {
            return Err("no episode has levels".into());
        }
        Ok(list)
    }
}

/// Reads `assets/episode.ron`, panicking with the path on failure.
pub fn load_episodes() -> EpisodeList {
    let path = assets_dir().join("episode.ron");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    EpisodeList::from_ron(&src).unwrap_or_else(|e| panic!("invalid {}: {e}", path.display()))
}

/// Progress through the episode. Absent for direct-level runs.
#[derive(Resource, Debug)]
pub struct Episode {
    /// The episode being played (or, on the title screen, the one last chosen).
    pub def: EpisodeDef,
    /// Every episode the menu offers, in order; `def` is a copy of `choices[chosen]`.
    pub choices: Vec<EpisodeDef>,
    /// Index of `def` in `choices`.
    pub chosen: usize,
    /// Index into the level list of the level being played.
    pub index: usize,
    /// Loadout each player (index = `PlayerId`) enters the current level with; `None` = the
    /// starting loadout.
    pub entries: [Option<CarryOver>; MAX_PLAYERS],
    /// Stats of the finished levels.
    pub totals: LevelStats,
    /// Pre-built maps used instead of loading `def.levels` (test/demo hook).
    pub maps: Option<Vec<Map>>,
}

impl Episode {
    /// A single-episode run (the menu offers just this one).
    pub fn new(def: EpisodeDef) -> Self {
        Self::with_choices(vec![def])
    }

    /// The menu's episodes; the first playable one is chosen (the first one when none is, as in
    /// tests that set `maps`). Panics on an empty list.
    pub fn with_choices(choices: Vec<EpisodeDef>) -> Self {
        assert!(!choices.is_empty(), "an episode list with no episodes");
        let chosen = choices.iter().position(EpisodeDef::playable).unwrap_or(0);
        Episode {
            def: choices[chosen].clone(),
            choices,
            chosen,
            index: 0,
            entries: Default::default(),
            totals: LevelStats::default(),
            maps: None,
        }
    }

    /// Chooses episode `i` of `choices` (unplayable or out-of-range choices are ignored) and
    /// rewinds to its first level. Returns whether it was chosen.
    pub fn choose(&mut self, i: usize) -> bool {
        match self.choices.get(i) {
            Some(def) if def.playable() => {
                self.def = def.clone();
                self.chosen = i;
                self.index = 0;
                self.entries = Default::default();
                self.totals = LevelStats::default();
                true
            }
            _ => false,
        }
    }

    /// Player `id` left: later players' entry loadouts and kill tallies move down one slot.
    pub fn remove_player(&mut self, id: PlayerId) {
        let i = id.0 as usize;
        if i < MAX_PLAYERS {
            self.entries[i..].rotate_left(1);
            self.entries[MAX_PLAYERS - 1] = None;
        }
        self.totals.remove_player(id);
    }

    /// Number of levels: the test maps when set, else the episode file's list.
    pub fn len(&self) -> usize {
        self.maps.as_ref().map_or(self.def.levels.len(), Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The file name of level `i` (the map's own name when test `maps` are set).
    pub fn file(&self, i: usize) -> String {
        match &self.maps {
            Some(maps) => maps[i].name.clone(),
            None => self.def.levels[i].clone(),
        }
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
    let (file, map) = {
        let mut ep = world.resource_mut::<Episode>();
        ep.index = 0;
        ep.entries = Default::default();
        ep.totals = LevelStats::default();
        (ep.file(0), ep.map(0))
    };
    world.insert_resource(LevelFile(file));
    load_level(world, map);
}

/// After a completed level: carry the loadout on and load the next level, or end the episode.
pub fn advance(world: &mut World) {
    let mut entries: [Option<CarryOver>; MAX_PLAYERS] = Default::default();
    let mut q = world.query::<(
        &Player,
        &PlayerVitals,
        &PlayerArsenal,
        &PlayerInventory,
        Option<&EntryLoadout>,
    )>();
    for (p, v, a, i, entry) in q.iter(world) {
        // A player who is down goes on with what they entered the level with.
        let carry = match entry {
            Some(e) if v.0.health.hp <= 0 => e.0.clone(),
            _ => CarryOver::take(&v.0, &a.0, &i.0),
        };
        if let Some(slot) = entries.get_mut(p.id.0 as usize) {
            *slot = Some(carry);
        }
    }
    let stats = world.resource::<Stats>().0;
    let next = {
        let mut ep = world.resource_mut::<Episode>();
        ep.totals.add(&stats);
        ep.entries = entries;
        ep.index += 1;
        (ep.index < ep.len()).then(|| (ep.file(ep.index), ep.map(ep.index)))
    };
    match next {
        Some((file, map)) => {
            world.insert_resource(LevelFile(file));
            load_level(world, map)
        }
        None => *world.resource_mut::<PlayState>() = PlayState::EpisodeEnd,
    }
}

/// Level stats: reset at every spawn, fed by the fixed tick.
pub struct EpisodePlugin;

impl Plugin for EpisodePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(CoopPlugin)
            .init_resource::<Stats>()
            .add_systems(SpawnLevel, reset_stats.after(crate::combat::spawn_combat))
            .add_systems(FixedUpdate, record_stats.after(CombatSet).run_if(sim_ticks));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, levels: &[&str]) -> EpisodeDef {
        EpisodeDef {
            name: name.into(),
            levels: levels.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn list_parses_and_needs_a_playable_episode() {
        let list = EpisodeList::from_ron(
            r#"(episodes: [(name: "A", levels: ["a.ron"]), (name: "B", levels: [])])"#,
        )
        .unwrap();
        assert_eq!(list.episodes.len(), 2);
        assert!(list.episodes[0].playable() && !list.episodes[1].playable());
        let err = EpisodeList::from_ron(r#"(episodes: [(name: "B", levels: [])])"#);
        assert!(err.unwrap_err().contains("no episode has levels"));
        assert!(EpisodeList::from_ron(r#"(name: "old", levels: ["a.ron"])"#).is_err());
    }

    #[test]
    fn choosing_switches_episode_and_skips_unplayable() {
        let mut ep = Episode::with_choices(vec![
            def("Soon", &[]),
            def("A", &["a.ron"]),
            def("B", &["b1.ron", "b2.ron"]),
        ]);
        assert_eq!((ep.chosen, ep.def.name.as_str()), (1, "A"));
        ep.index = 1;
        assert!(ep.choose(2));
        assert_eq!((ep.chosen, ep.def.name.as_str(), ep.index), (2, "B", 0));
        assert_eq!(ep.len(), 2);
        assert!(!ep.choose(0));
        assert!(!ep.choose(9));
        assert_eq!(ep.chosen, 2);
    }
}
