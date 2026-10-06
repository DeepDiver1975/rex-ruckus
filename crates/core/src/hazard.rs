//! Damaging floors: slime and live electric grates hurt the player standing on them.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum HazardKind {
    Slime,
    Electric,
}

/// A sector's damaging floor: `damage` (before difficulty and armour) every `interval` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Hazard {
    pub damage: i32,
    pub interval: f32,
    pub kind: HazardKind,
}
