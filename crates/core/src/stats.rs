//! Per-level tallies for the end-of-level screen: kills, secrets, time.

use crate::combat::{Combat, CombatEvent};
use crate::map::ActorKind;

/// Kills, secrets and time for one level (or, summed with `add`, an episode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LevelStats {
    pub kills: u32,
    pub kills_total: u32,
    pub secrets: u32,
    pub secrets_total: u32,
    /// Fixed ticks spent playing.
    pub ticks: u32,
}

/// Barrels are props, not enemies: they never count as kills.
fn counts(kind: ActorKind) -> bool {
    kind != ActorKind::Barrel
}

impl LevelStats {
    /// Totals for a freshly spawned level (actors already filtered by difficulty).
    pub fn new(combat: &Combat) -> Self {
        let (secrets, secrets_total) = combat.destruct.secrets();
        LevelStats {
            kills_total: combat.actors.iter().filter(|a| counts(a.kind)).count() as u32,
            secrets,
            secrets_total,
            ..Default::default()
        }
    }

    /// Folds one tick's combat events into the tallies and refreshes the secrets found.
    pub fn record(&mut self, combat: &Combat, events: &[CombatEvent]) {
        for ev in events {
            if let CombatEvent::ActorKilled(i) = ev
                && combat.actors.get(*i).is_some_and(|a| counts(a.kind))
            {
                self.kills += 1;
            }
        }
        self.secrets = combat.destruct.secrets().0;
    }

    /// Counts one fixed tick of play.
    pub fn tick(&mut self) {
        self.ticks += 1;
    }

    /// Play time in seconds at `hz` ticks per second.
    pub fn seconds(&self, hz: f32) -> f32 {
        self.ticks as f32 / hz
    }

    /// Adds another level's tallies (episode totals).
    pub fn add(&mut self, o: &LevelStats) {
        self.kills += o.kills;
        self.kills_total += o.kills_total;
        self.secrets += o.secrets;
        self.secrets_total += o.secrets_total;
        self.ticks += o.ticks;
    }
}

/// `m:ss`.
pub fn format_time(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{Combat, CombatEvent};
    use crate::difficulty::Difficulty;
    use crate::fixtures::{combat_room, defs};
    use crate::map::{ActorKind, ActorSpawn};
    use glam::Vec2;

    fn spawn(kind: ActorKind, x: f32) -> ActorSpawn {
        ActorSpawn {
            kind,
            pos: Vec2::new(x, 6.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
        }
    }

    #[test]
    fn counts_kills_but_not_barrels() {
        let mut map = combat_room();
        map.actors.push(spawn(ActorKind::Grunt, 2.0));
        map.actors.push(spawn(ActorKind::Barrel, 4.0));
        let combat = Combat::spawn(&map, &defs(), 1);
        let mut s = LevelStats::new(&combat);
        assert_eq!(s.kills_total, 1);
        let grunt = combat
            .actors
            .iter()
            .position(|a| a.kind == ActorKind::Grunt)
            .unwrap();
        let barrel = combat
            .actors
            .iter()
            .position(|a| a.kind == ActorKind::Barrel)
            .unwrap();
        s.record(
            &combat,
            &[
                CombatEvent::ActorKilled(barrel),
                CombatEvent::ActorKilled(grunt),
            ],
        );
        assert_eq!(s.kills, 1);
    }

    #[test]
    fn time_formats_minutes_and_seconds() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(125.4), "2:05");
    }

    #[test]
    fn add_sums_and_seconds_uses_hz() {
        let mut a = LevelStats {
            kills: 1,
            kills_total: 2,
            secrets: 0,
            secrets_total: 1,
            ticks: 60,
        };
        a.add(&LevelStats {
            kills: 3,
            kills_total: 3,
            secrets: 1,
            secrets_total: 1,
            ticks: 30,
        });
        assert_eq!(
            (a.kills, a.kills_total, a.secrets, a.secrets_total),
            (4, 5, 1, 2)
        );
        assert_eq!(a.seconds(30.0), 3.0);
    }
}
