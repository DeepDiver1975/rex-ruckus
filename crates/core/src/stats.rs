//! Per-level tallies for the end-of-level screen: kills, secrets, time.

use crate::combat::{Combat, CombatEvent};
use crate::map::ActorKind;
use crate::players::{MAX_PLAYERS, PlayerId};
use crate::projectile::Shooter;
use serde::{Deserialize, Serialize};

/// Kills, secrets and time for one level (or, summed with `add`, an episode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LevelStats {
    pub kills: u32,
    /// Kills credited to each player (index = `PlayerId`); kills by anything else only count
    /// in `kills`.
    pub kills_by: [u32; MAX_PLAYERS],
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

    /// Folds one tick's combat events into the tallies and refreshes the secrets found. An
    /// actor that joins play (a boss's minion) adds to the total.
    pub fn record(&mut self, combat: &Combat, events: &[CombatEvent]) {
        for ev in events {
            let (CombatEvent::ActorKilled { actor, .. } | CombatEvent::ActorSpawned(actor)) = ev
            else {
                continue;
            };
            if !combat.actors.get(*actor).is_some_and(|a| counts(a.kind)) {
                continue;
            }
            if let CombatEvent::ActorKilled { by, .. } = ev {
                self.kills += 1;
                if let Shooter::Player(id) = by
                    && let Some(n) = self.kills_by.get_mut(id.0 as usize)
                {
                    *n += 1;
                }
            } else {
                self.kills_total += 1;
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

    /// A co-op seat left: later players' kills move down one slot, so ids stay dense.
    pub fn remove_player(&mut self, id: PlayerId) {
        let i = id.0 as usize;
        if i < MAX_PLAYERS {
            self.kills_by.copy_within(i + 1.., i);
            self.kills_by[MAX_PLAYERS - 1] = 0;
        }
    }

    /// Adds another level's tallies (episode totals).
    pub fn add(&mut self, o: &LevelStats) {
        self.kills += o.kills;
        for (a, b) in self.kills_by.iter_mut().zip(o.kills_by) {
            *a += b;
        }
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
    use crate::players::PlayerId;
    use crate::projectile::Shooter;
    use glam::Vec2;

    fn spawn(kind: ActorKind, x: f32) -> ActorSpawn {
        ActorSpawn {
            kind,
            pos: Vec2::new(x, 6.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
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
                CombatEvent::ActorKilled {
                    actor: barrel,
                    by: Shooter::Actor(grunt),
                },
                CombatEvent::ActorKilled {
                    actor: grunt,
                    by: Shooter::Player(PlayerId(0)),
                },
            ],
        );
        // Every death counts, whoever the killer; barrels never do.
        assert_eq!(s.kills, 1);
    }

    #[test]
    fn spawned_minions_add_to_the_total() {
        let mut map = combat_room();
        map.actors.push(spawn(ActorKind::Grunt, 2.0));
        map.actors.push(spawn(ActorKind::Slasher, 4.0)); // stands in for a minion
        let combat = Combat::spawn(&map, &defs(), 1);
        let mut s = LevelStats {
            kills_total: 1,
            ..LevelStats::default()
        };
        s.record(&combat, &[CombatEvent::ActorSpawned(1)]);
        assert_eq!(s.kills_total, 2);
        s.record(
            &combat,
            &[CombatEvent::ActorKilled {
                actor: 1,
                by: Shooter::Player(PlayerId(0)),
            }],
        );
        assert_eq!((s.kills, s.kills_total), (1, 2));
    }

    #[test]
    fn kills_are_credited_to_their_player() {
        let mut map = combat_room();
        map.actors.push(spawn(ActorKind::Grunt, 2.0));
        map.actors.push(spawn(ActorKind::Grunt, 4.0));
        let combat = Combat::spawn(&map, &defs(), 1);
        let mut s = LevelStats::new(&combat);
        s.record(
            &combat,
            &[
                CombatEvent::ActorKilled {
                    actor: 0,
                    by: Shooter::Player(PlayerId(1)),
                },
                CombatEvent::ActorKilled {
                    actor: 1,
                    by: Shooter::World,
                },
            ],
        );
        assert_eq!(s.kills, 2);
        assert_eq!(s.kills_by, [0, 1, 0, 0]);
    }

    #[test]
    fn out_of_range_player_ids_are_ignored() {
        let mut map = combat_room();
        map.actors.push(spawn(ActorKind::Grunt, 2.0));
        let combat = Combat::spawn(&map, &defs(), 1);
        let mut s = LevelStats::new(&combat);
        s.record(
            &combat,
            &[CombatEvent::ActorKilled {
                actor: 0,
                by: Shooter::Player(PlayerId(200)),
            }],
        );
        assert_eq!((s.kills, s.kills_by), (1, [0; MAX_PLAYERS]));
        s.remove_player(PlayerId(200));
        assert_eq!(s.kills_by, [0; MAX_PLAYERS]);
    }

    #[test]
    fn add_sums_kills_by_and_remove_player_shifts_down() {
        let mut a = LevelStats {
            kills_by: [3, 1, 4, 0],
            ..Default::default()
        };
        a.add(&LevelStats {
            kills_by: [1, 1, 1, 1],
            ..Default::default()
        });
        assert_eq!(a.kills_by, [4, 2, 5, 1]);
        a.remove_player(PlayerId(1));
        assert_eq!(a.kills_by, [4, 5, 1, 0]);
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
            ..Default::default()
        };
        a.add(&LevelStats {
            kills: 3,
            kills_total: 3,
            secrets: 1,
            secrets_total: 1,
            ticks: 30,
            ..Default::default()
        });
        assert_eq!(
            (a.kills, a.kills_total, a.secrets, a.secrets_total),
            (4, 5, 1, 2)
        );
        assert_eq!(a.seconds(30.0), 3.0);
    }
}
