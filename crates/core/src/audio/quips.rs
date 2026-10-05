//! Quip director: decides when the hero speaks a one-liner. Pure and deterministic; the game
//! sends triggers each frame and shows/plays whatever line comes back.

use crate::defs::WeaponId;
use crate::map::ActorKind;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Minimum seconds between quips (`LevelComplete` ignores it).
pub const COOLDOWN: f32 = 8.0;
/// Chance that a plain `Kill` is followed by a quip.
pub const KILL_CHANCE: f32 = 0.2;
/// Kills (3 or more) within this many seconds make a `MultiKill`.
pub const MULTI_KILL_WINDOW: f32 = 2.5;
/// Health below which `LowHealthWatch` fires.
pub const LOW_HEALTH: i32 = 25;
/// Health above which `LowHealthWatch` re-arms.
pub const REARM_HEALTH: i32 = 50;

const MULTI_KILL_COUNT: usize = 3;

/// A moment the hero may comment on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuipTrigger {
    LevelStart,
    Kill(ActorKind),
    MultiKill,
    BigKill,
    LowHealth,
    NewWeapon(WeaponId),
    Secret,
    NeedKey,
    LevelComplete,
    Respawn,
}

/// A trigger without its payload: the key quips are filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuipOn {
    LevelStart,
    Kill,
    MultiKill,
    BigKill,
    LowHealth,
    NewWeapon,
    Secret,
    NeedKey,
    LevelComplete,
    Respawn,
}

impl QuipOn {
    pub const ALL: [QuipOn; 10] = [
        QuipOn::LevelStart,
        QuipOn::Kill,
        QuipOn::MultiKill,
        QuipOn::BigKill,
        QuipOn::LowHealth,
        QuipOn::NewWeapon,
        QuipOn::Secret,
        QuipOn::NeedKey,
        QuipOn::LevelComplete,
        QuipOn::Respawn,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&o| o == self).unwrap()
    }
}

impl QuipTrigger {
    pub fn on(self) -> QuipOn {
        match self {
            Self::LevelStart => QuipOn::LevelStart,
            Self::Kill(_) => QuipOn::Kill,
            Self::MultiKill => QuipOn::MultiKill,
            Self::BigKill => QuipOn::BigKill,
            Self::LowHealth => QuipOn::LowHealth,
            Self::NewWeapon(_) => QuipOn::NewWeapon,
            Self::Secret => QuipOn::Secret,
            Self::NeedKey => QuipOn::NeedKey,
            Self::LevelComplete => QuipOn::LevelComplete,
            Self::Respawn => QuipOn::Respawn,
        }
    }
}

/// Highest priority first.
const PRIORITY: [QuipOn; 10] = [
    QuipOn::LevelComplete,
    QuipOn::MultiKill,
    QuipOn::BigKill,
    QuipOn::NewWeapon,
    QuipOn::Secret,
    QuipOn::LowHealth,
    QuipOn::NeedKey,
    QuipOn::Respawn,
    QuipOn::LevelStart,
    QuipOn::Kill,
];

/// One spoken line. The audio is `assets/quips/<id>.ogg`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quip {
    pub id: String,
    pub on: QuipOn,
    pub text: String,
}

#[derive(Debug, Error, PartialEq)]
pub enum QuipError {
    #[error("quip table parse error: {0}")]
    Parse(String),
    #[error("quip id {0:?} must match [a-z0-9_]+")]
    BadId(String),
    #[error("duplicate quip id {0:?}")]
    DuplicateId(String),
}

/// All quips, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuipTable {
    pub quips: Vec<Quip>,
}

impl QuipTable {
    pub fn parse(src: &str) -> Result<QuipTable, QuipError> {
        let quips: Vec<Quip> = ron::from_str(src).map_err(|e| QuipError::Parse(e.to_string()))?;
        let mut seen = HashSet::new();
        for q in &quips {
            let ok = !q.id.is_empty()
                && q.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !ok {
                return Err(QuipError::BadId(q.id.clone()));
            }
            if !seen.insert(q.id.as_str()) {
                return Err(QuipError::DuplicateId(q.id.clone()));
            }
        }
        Ok(QuipTable { quips })
    }
}

/// Hysteresis for the `LowHealth` trigger: fires once on dropping below `LOW_HEALTH`, then stays
/// quiet until health has climbed back above `REARM_HEALTH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LowHealthWatch {
    pub armed: bool,
}

impl Default for LowHealthWatch {
    fn default() -> Self {
        Self { armed: true }
    }
}

impl LowHealthWatch {
    pub fn update(&mut self, hp: i32) -> bool {
        if self.armed && hp < LOW_HEALTH {
            self.armed = false;
            return true;
        }
        if hp > REARM_HEALTH {
            self.armed = true;
        }
        false
    }
}

/// Per-`QuipOn` shuffled cycle through that pool's line indices.
#[derive(Debug, Clone, Default)]
struct Pool {
    lines: Vec<usize>,
    order: Vec<usize>,
    next: usize,
    last: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct QuipDirector {
    table: QuipTable,
    pools: Vec<Pool>,
    rng: Rng,
    last_played: Option<f32>,
    kills: Vec<f32>,
}

impl QuipDirector {
    pub fn new(table: QuipTable, seed: u64) -> Self {
        let mut pools = vec![Pool::default(); QuipOn::ALL.len()];
        for (i, q) in table.quips.iter().enumerate() {
            pools[q.on.index()].lines.push(i);
        }
        Self {
            table,
            pools,
            rng: Rng::new(seed),
            last_played: None,
            kills: Vec::new(),
        }
    }

    /// Feed this frame's triggers at level time `now` (seconds). Returns the quip to play, if any.
    pub fn update(&mut self, now: f32, triggers: &[QuipTrigger]) -> Option<&Quip> {
        let mut present = [false; 10];
        for t in triggers {
            match t {
                // Barrels never count as kills.
                QuipTrigger::Kill(ActorKind::Barrel) => {}
                QuipTrigger::Kill(_) => {
                    self.kills.push(now);
                    present[QuipOn::Kill.index()] = true;
                }
                other => present[other.on().index()] = true,
            }
        }
        self.kills.retain(|&t| now - t <= MULTI_KILL_WINDOW);
        if self.kills.len() >= MULTI_KILL_COUNT {
            self.kills.clear();
            present[QuipOn::MultiKill.index()] = true;
        }

        let on = PRIORITY
            .into_iter()
            .find(|o| present[o.index()] && !self.pools[o.index()].lines.is_empty())?;
        if on != QuipOn::LevelComplete && self.last_played.is_some_and(|t| now - t < COOLDOWN) {
            return None;
        }
        if on == QuipOn::Kill && !self.rng.chance(KILL_CHANCE) {
            return None;
        }
        let line = self.draw(on);
        self.last_played = Some(now);
        Some(&self.table.quips[line])
    }

    /// Next line of the pool, reshuffling when used up and never repeating across the seam.
    fn draw(&mut self, on: QuipOn) -> usize {
        let pool = &mut self.pools[on.index()];
        if pool.next >= pool.order.len() {
            pool.order = pool.lines.clone();
            for i in (1..pool.order.len()).rev() {
                let j = (self.rng.next_u64() % (i as u64 + 1)) as usize;
                pool.order.swap(i, j);
            }
            if pool.order.len() > 1 && Some(pool.order[0]) == pool.last {
                pool.order.swap(0, 1);
            }
            pool.next = 0;
        }
        let line = pool.order[pool.next];
        pool.next += 1;
        pool.last = Some(line);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn table(n: usize) -> QuipTable {
        let mut quips = Vec::new();
        for on in QuipOn::ALL {
            for i in 0..n {
                quips.push(Quip {
                    id: format!("{on:?}_{i}").to_lowercase(),
                    on,
                    text: format!("{on:?} {i}"),
                });
            }
        }
        QuipTable { quips }
    }

    fn director(seed: u64) -> QuipDirector {
        QuipDirector::new(table(3), seed)
    }

    const K: QuipTrigger = QuipTrigger::Kill(ActorKind::Grunt);

    #[test]
    fn cooldown_blocks_then_allows_and_level_complete_bypasses() {
        let mut d = director(1);
        assert!(d.update(0.0, &[QuipTrigger::Secret]).is_some());
        assert!(d.update(7.9, &[QuipTrigger::Secret]).is_none());
        assert!(d.update(8.0, &[QuipTrigger::Secret]).is_some());
        assert!(d.update(9.0, &[QuipTrigger::Secret]).is_none());
        let q = d.update(9.0, &[QuipTrigger::LevelComplete]).unwrap();
        assert_eq!(q.on, QuipOn::LevelComplete);
    }

    #[test]
    fn three_quick_kills_make_a_multi_kill() {
        let mut d = director(1);
        d.update(0.0, &[K]);
        d.update(1.0, &[K]);
        // Not a quip-eligible kill roll necessarily, but MultiKill is certain.
        let q = d.update(2.0, &[K]).unwrap();
        assert_eq!(q.on, QuipOn::MultiKill);
    }

    #[test]
    fn slow_kills_and_barrels_never_multi_kill() {
        let mut d = director(1);
        for i in 0..3 {
            let q = d.update(i as f32 * 3.0, &[K]);
            assert!(q.is_none_or(|q| q.on == QuipOn::Kill));
        }
        let mut d = director(1);
        let b = QuipTrigger::Kill(ActorKind::Barrel);
        for _ in 0..5 {
            assert!(d.update(0.0, &[b, b, b]).is_none());
        }
    }

    #[test]
    fn multi_kill_fires_once_per_window() {
        let mut d = director(1);
        d.update(0.0, &[K, K, K]).unwrap();
        // Recorded kills were cleared, so two more are not enough.
        assert!(d.update(10.0, &[K, K]).is_none_or(|q| q.on == QuipOn::Kill));
    }

    #[test]
    fn priority_multi_kill_beats_kill() {
        let mut d = director(1);
        let q = d.update(0.0, &[K, K, K]).unwrap();
        assert_eq!(q.on, QuipOn::MultiKill);
        let mut d = director(1);
        let q = d
            .update(0.0, &[K, QuipTrigger::Respawn, QuipTrigger::BigKill])
            .unwrap();
        assert_eq!(q.on, QuipOn::BigKill);
    }

    #[test]
    fn missing_pool_falls_to_next_priority() {
        let mut t = table(2);
        t.quips.retain(|q| q.on != QuipOn::BigKill);
        let mut d = QuipDirector::new(t, 1);
        let q = d
            .update(0.0, &[QuipTrigger::BigKill, QuipTrigger::Secret])
            .unwrap();
        assert_eq!(q.on, QuipOn::Secret);
    }

    #[test]
    fn kill_chance_rate() {
        let mut played = 0;
        let trials = 2000;
        let mut d = director(7);
        for i in 0..trials {
            // Space calls far apart so neither cooldown nor multi-kill interferes.
            if d.update(i as f32 * 100.0, &[K]).is_some() {
                played += 1;
            }
        }
        let rate = played as f32 / trials as f32;
        assert!((0.1..=0.3).contains(&rate), "rate {rate}");
    }

    #[test]
    fn no_repeats_until_pool_used_up() {
        let mut d = QuipDirector::new(table(4), 3);
        let mut last = None;
        for round in 0..5 {
            let mut ids = HashSet::new();
            for i in 0..4 {
                let now = (round * 4 + i) as f32 * 10.0;
                let q = d.update(now, &[QuipTrigger::Secret]).unwrap();
                assert!(ids.insert(q.id.clone()), "repeat within cycle");
                if i == 0 {
                    assert_ne!(last.as_ref(), Some(&q.id), "repeat across reshuffle");
                }
                last = Some(q.id.clone());
            }
        }
    }

    #[test]
    fn same_seed_same_output() {
        let run = |seed| {
            let mut d = director(seed);
            (0..200)
                .map(|i| {
                    let trig =
                        [K, QuipTrigger::Secret, QuipTrigger::NeedKey][i % 3..i % 3 + 1].to_vec();
                    d.update(i as f32 * 1.5, &trig).map(|q| q.id.clone())
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(5), run(5));
    }

    #[test]
    fn low_health_hysteresis() {
        let mut w = LowHealthWatch::default();
        assert!(!w.update(80));
        assert!(w.update(24));
        assert!(!w.update(10));
        assert!(!w.update(40));
        assert!(!w.update(50));
        assert!(!w.update(20), "not re-armed until above REARM_HEALTH");
        assert!(!w.update(51));
        assert!(w.update(24));
        assert!(!w.update(25));
    }

    #[test]
    fn parse_errors() {
        let dup = r#"[(id: "a", on: Kill, text: "x"), (id: "a", on: Secret, text: "y")]"#;
        assert_eq!(
            QuipTable::parse(dup),
            Err(QuipError::DuplicateId("a".into()))
        );
        let bad = r#"[(id: "Bad-Id", on: Kill, text: "x")]"#;
        assert_eq!(
            QuipTable::parse(bad),
            Err(QuipError::BadId("Bad-Id".into()))
        );
        assert!(matches!(
            QuipTable::parse("[(id:"),
            Err(QuipError::Parse(_))
        ));
    }

    #[test]
    fn real_table_parses_and_covers_every_trigger() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/quips/quips.ron");
        let t = QuipTable::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        for on in QuipOn::ALL {
            let n = t.quips.iter().filter(|q| q.on == on).count();
            assert!(n >= 2, "{on:?} has {n} lines");
        }
        for q in &t.quips {
            assert!(!q.text.is_empty());
        }
    }

    fn trigger_strategy() -> impl Strategy<Value = QuipTrigger> {
        prop_oneof![
            Just(QuipTrigger::LevelStart),
            Just(K),
            Just(QuipTrigger::Kill(ActorKind::Barrel)),
            Just(QuipTrigger::MultiKill),
            Just(QuipTrigger::BigKill),
            Just(QuipTrigger::LowHealth),
            Just(QuipTrigger::NewWeapon(WeaponId::Shotgun)),
            Just(QuipTrigger::Secret),
            Just(QuipTrigger::NeedKey),
            Just(QuipTrigger::LevelComplete),
            Just(QuipTrigger::Respawn),
        ]
    }

    proptest! {
        #[test]
        fn quips_respect_cooldown(
            seed in any::<u64>(),
            steps in prop::collection::vec(
                (0.0f32..3.0, prop::collection::vec(trigger_strategy(), 0..4)), 0..200),
        ) {
            let mut d = director(seed);
            let mut now = 0.0f32;
            let mut prev: Option<f32> = None;
            for (dt, trig) in steps {
                now += dt;
                let played = d.update(now, &trig).map(|q| q.on);
                if played.is_some_and(|on| on != QuipOn::LevelComplete) {
                    if let Some(p) = prev {
                        prop_assert!(now - p >= COOLDOWN, "{} then {}", p, now);
                    }
                    prev = Some(now);
                }
            }
        }
    }
}
