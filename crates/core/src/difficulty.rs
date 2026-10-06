//! Skill levels: which authored actors and items appear, and how hard enemies hit.

use crate::map::Map;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Minimum skill an actor or item needs to appear (`Easy` = always), and the chosen skill.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    /// Multiplier on damage the player takes.
    pub fn damage_scale(self) -> f32 {
        match self {
            Difficulty::Easy => 0.6,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 1.4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
        }
    }
}

impl FromStr for Difficulty {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Difficulty::ALL
            .into_iter()
            .find(|d| d.label().eq_ignore_ascii_case(s))
            .ok_or_else(|| format!("unknown difficulty '{s}' (easy, normal, hard)"))
    }
}

impl Map {
    /// The map as played on `d`: actors and items whose `skill` is above `d` are left out.
    pub fn for_difficulty(&self, d: Difficulty) -> Map {
        let mut m = self.clone();
        m.actors.retain(|a| a.skill <= d);
        m.items.retain(|i| i.skill <= d);
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Map;

    const LEVEL: &str = r#"(
        name: "skill", materials: ["concrete"],
        vertices: [(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)],
        sectors: [(loops: [[0, 1, 2, 3]], floor_z: 0.0, ceil_z: 3.0,
                   floor_mat: 0, ceil_mat: 0, wall_mat: 0)],
        player_start: (pos: (1.0, 1.0), angle_deg: 0.0),
        items: [(kind: Shotgun, pos: (2.0, 2.0)),
                (kind: Armour, pos: (3.0, 3.0), skill: Normal)],
        actors: [(kind: Grunt, pos: (5.0, 5.0)),
                 (kind: Enforcer, pos: (6.0, 6.0), skill: Hard)],
    )"#;

    #[test]
    fn untagged_entries_appear_on_every_skill() {
        let map = Map::from_ron(LEVEL).unwrap();
        let easy = map.for_difficulty(Difficulty::Easy);
        assert_eq!((easy.items.len(), easy.actors.len()), (1, 1));
        let normal = map.for_difficulty(Difficulty::Normal);
        assert_eq!((normal.items.len(), normal.actors.len()), (2, 1));
        let hard = map.for_difficulty(Difficulty::Hard);
        assert_eq!((hard.items.len(), hard.actors.len()), (2, 2));
    }

    #[test]
    fn damage_scale_per_skill() {
        assert_eq!(Difficulty::Easy.damage_scale(), 0.6);
        assert_eq!(Difficulty::Normal.damage_scale(), 1.0);
        assert_eq!(Difficulty::Hard.damage_scale(), 1.4);
    }

    #[test]
    fn parses_cli_names() {
        assert_eq!("hard".parse::<Difficulty>(), Ok(Difficulty::Hard));
        assert_eq!("Easy".parse::<Difficulty>(), Ok(Difficulty::Easy));
        assert!("nightmare".parse::<Difficulty>().is_err());
    }
}
