//! Damaging floors: slime and live electric grates hurt the player standing on them.

use serde::{Deserialize, Serialize};

use crate::collide::Body;
use crate::map::Map;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HazardKind {
    Slime,
    Electric,
}

/// A sector's damaging floor: `damage` (before difficulty and armour) every `interval` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hazard {
    pub damage: i32,
    pub interval: f32,
    pub kind: HazardKind,
}

/// Feet within this height of a hazard floor touch it.
pub const HAZARD_CONTACT: f32 = 0.05;

/// When the player's next burn is due.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HazardClock {
    wait: Option<f32>,
}

impl HazardClock {
    /// One tick with `contact` (the hazard the player touches, if any). Returns the hazard when
    /// it burns now: at once on first touch, then every `interval` while touching. The
    /// countdown keeps running out of contact, so hopping neither dodges nor doubles a burn.
    pub fn tick(&mut self, contact: Option<Hazard>, dt: f32) -> Option<Hazard> {
        let wait = self.wait.map(|w| w - dt);
        match (contact, wait) {
            (Some(h), None) => {
                self.wait = Some(h.interval);
                Some(h)
            }
            (Some(h), Some(w)) if w <= 0.0 => {
                self.wait = Some(w + h.interval);
                Some(h)
            }
            (_, w) => {
                self.wait = w.filter(|&w| w > 0.0);
                None
            }
        }
    }
}

/// The hazard `body` stands on: its sector has one, the body is on the ground, and its feet are
/// within `HAZARD_CONTACT` of the live floor (lifts move it). Jetpack flight never touches.
pub fn contact(map: &Map, body: &Body) -> Option<Hazard> {
    let sec = &map.sectors[body.sector];
    let h = sec.hazard?;
    (body.on_ground && (body.pos.z - sec.floor_z).abs() <= HAZARD_CONTACT).then_some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const SLIME: Hazard = Hazard {
        damage: 4,
        interval: 0.75,
        kind: HazardKind::Slime,
    };

    #[test]
    fn first_touch_burns_at_once_then_every_interval() {
        let mut c = HazardClock::default();
        assert_eq!(c.tick(Some(SLIME), DT), Some(SLIME));
        let burns = (0..100)
            .filter(|_| c.tick(Some(SLIME), DT).is_some())
            .count(); // 1.67 s
        assert_eq!(burns, 2);
    }

    #[test]
    fn hopping_out_and_back_does_not_rearm_the_first_burn() {
        let mut c = HazardClock::default();
        c.tick(Some(SLIME), DT);
        for _ in 0..10 {
            assert_eq!(c.tick(None, DT), None);
        }
        assert_eq!(c.tick(Some(SLIME), DT), None, "still inside the interval");
        for _ in 0..60 {
            c.tick(None, DT);
        }
        assert_eq!(
            c.tick(Some(SLIME), DT),
            Some(SLIME),
            "re-armed after a full interval out"
        );
    }
}
