//! Gag props: toilets, vending machines and pool tables. Solid oriented boxes the player bumps
//! into and can use; shots and pathfinding ignore them (a documented limitation).

use crate::map::SectorId;
use glam::Vec2;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum PropKind {
    Toilet,
    Vending,
    PoolTable,
}

impl PropKind {
    pub const ALL: [PropKind; 3] = [PropKind::Toilet, PropKind::Vending, PropKind::PoolTable];

    /// Half the footprint: x along the facing (depth), y across it (width), metres.
    pub fn half_extents(self) -> Vec2 {
        match self {
            PropKind::Toilet => Vec2::new(0.35, 0.25),
            PropKind::Vending => Vec2::new(0.4, 0.5),
            PropKind::PoolTable => Vec2::new(1.2, 0.65),
        }
    }

    pub fn height(self) -> f32 {
        match self {
            PropKind::Toilet => 0.8,
            PropKind::Vending => 1.9,
            PropKind::PoolTable => 0.85,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PropKind::Toilet => "toilet",
            PropKind::Vending => "vending machine",
            PropKind::PoolTable => "pool table",
        }
    }
}

fn default_stock() -> u32 {
    3
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawProp {
    pub kind: PropKind,
    pub pos: (f32, f32),
    /// Where the front faces (core degrees, 0 = east, CCW).
    #[serde(default)]
    pub angle_deg: f32,
    /// Sodas in a vending machine; ignored by other kinds.
    #[serde(default = "default_stock")]
    pub stock: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub kind: PropKind,
    pub pos: Vec2,
    /// Radians, 0 = +x, CCW.
    pub angle: f32,
    pub stock: u32,
    /// The sector its centre stands in (found at load).
    pub sector: SectorId,
}

impl Prop {
    /// `p` in the prop's frame: x along its facing, y to its left.
    pub fn to_local(&self, p: Vec2) -> Vec2 {
        Vec2::from_angle(-self.angle).rotate(p - self.pos)
    }

    /// The footprint's corners in world XY, counter-clockwise.
    pub fn corners(&self) -> [Vec2; 4] {
        let h = self.kind.half_extents();
        let r = Vec2::from_angle(self.angle);
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| self.pos + r.rotate(Vec2::new(x * h.x, y * h.y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_follow_the_facing() {
        let p = Prop {
            kind: PropKind::Toilet,
            pos: Vec2::new(14.0, 3.4),
            angle: 270f32.to_radians(),
            stock: 0,
            sector: 0,
        };
        // Facing south: the 0.7 m depth runs along y, the 0.5 m width along x.
        let (lo, hi) = p
            .corners()
            .iter()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), c| {
                (lo.min(*c), hi.max(*c))
            });
        assert!(
            (lo - Vec2::new(13.75, 3.05)).length() < 1e-4
                && (hi - Vec2::new(14.25, 3.75)).length() < 1e-4
        );
        assert!((p.to_local(Vec2::new(14.0, 3.0)) - Vec2::new(0.4, 0.0)).length() < 1e-5);
    }
}
