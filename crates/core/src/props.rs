//! Gag props: toilets, vending machines and pool tables. Solid oriented boxes the player bumps
//! into and can use; shots and pathfinding ignore them (a documented limitation).

use crate::collide::{Body, clip_move};
use crate::map::{Map, SectorId};
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

/// What using a prop did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropOutcome {
    /// Toilet: health gained.
    Healed(i32),
    /// Toilet at full health: just a flush.
    Flushed,
    /// Vending machine: a soda, health gained (0 at full health).
    Dispensed(i32),
    SoldOut,
    /// Pool table: flavour only.
    Racked,
    /// Still flushing (cooldown); nothing happens and no event is queued.
    Busy,
}

/// The XY push that moves a circle of `radius` at `p` clear of `prop`'s box, if they overlap:
/// away from the nearest point of the box, or out through the nearest face when the centre is
/// inside it.
pub fn circle_out_of_box(prop: &Prop, p: Vec2, radius: f32) -> Option<Vec2> {
    let half = prop.kind.half_extents();
    let local = prop.to_local(p);
    let d = local - local.clamp(-half, half);
    let push = if d != Vec2::ZERO {
        let dist = d.length();
        if dist >= radius {
            return None;
        }
        d / dist * (radius - dist)
    } else {
        let gap = half - local.abs();
        if gap.x < gap.y {
            Vec2::new((gap.x + radius) * local.x.signum(), 0.0)
        } else {
            Vec2::new(0.0, (gap.y + radius) * local.y.signum())
        }
    };
    Some(Vec2::from_angle(prop.angle).rotate(push))
}

/// Pushes `body` out of every prop box whose height range it overlaps, through `clip_move`
/// so walls still hold it. Called at the end of `step_player` and `step_flyer`.
pub fn push_out_of_props(map: &Map, body: &mut Body, step: f32) {
    for prop in &map.props {
        let floor = map.sectors[prop.sector].floor_z;
        if body.pos.z >= floor + prop.kind.height() || body.pos.z + body.height <= floor {
            continue;
        }
        if let Some(push) = circle_out_of_box(prop, body.pos.truncate(), body.radius) {
            clip_move(map, body, push, step);
        }
    }
}

/// Distance along the XY ray from `origin` (unit `dir`) to the first prop box it enters within
/// `max`, with the prop's index. A ray starting inside a box ignores that box.
pub fn ray_prop(map: &Map, origin: Vec2, dir: Vec2, max: f32) -> Option<(f32, usize)> {
    let mut best: Option<(f32, usize)> = None;
    for (i, prop) in map.props.iter().enumerate() {
        let half = prop.kind.half_extents();
        let o = prop.to_local(origin);
        if o.abs().cmple(half).all() {
            continue;
        }
        let d = Vec2::from_angle(-prop.angle).rotate(dir);
        let (mut t0, mut t1) = (0.0_f32, max);
        for (o, d, h) in [(o.x, d.x, half.x), (o.y, d.y, half.y)] {
            if d.abs() < 1e-6 {
                if o.abs() > h {
                    t0 = f32::INFINITY;
                }
            } else {
                let (a, b) = ((-h - o) / d, (h - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 <= t1 && best.is_none_or(|(t, _)| t0 < t) {
            best = Some((t0, i));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::Body;
    use crate::fixtures::engine_room;

    #[test]
    fn bodies_are_pushed_out_of_boxes_whatever_their_facing() {
        let map = engine_room("");
        // Pool table (facing east): its long side ends at y = 1.85.
        let mut b = Body::spawn(&map, Vec2::new(16.0, 2.0), 0.35, 1.8).unwrap();
        push_out_of_props(&map, &mut b, 0.55);
        assert!((b.pos.y - 2.2).abs() < 1e-3, "{:?}", b.pos);
        // Toilet facing south: x in [13.75, 14.25].
        let mut b = Body::spawn(&map, Vec2::new(14.5, 3.4), 0.35, 1.8).unwrap();
        push_out_of_props(&map, &mut b, 0.55);
        assert!((b.pos.x - 14.6).abs() < 1e-3, "{:?}", b.pos);
        // Standing above a prop's top: no push.
        let mut b = Body::spawn(&map, Vec2::new(16.0, 2.0), 0.35, 1.8).unwrap();
        b.pos.z = 1.0;
        push_out_of_props(&map, &mut b, 0.55);
        assert_eq!(b.pos.y, 2.0);
    }

    #[test]
    fn walking_into_the_pool_table_stops_at_it() {
        use crate::movement::{MoveInput, Tuning, step_player};
        let map = engine_room("");
        let mut b = Body::spawn(&map, Vec2::new(16.0, 3.0), 0.35, 1.8).unwrap();
        let input = MoveInput {
            wish: Vec2::new(0.0, -1.0),
            ..MoveInput::default()
        };
        for _ in 0..120 {
            step_player(&map, &mut b, &input, &Tuning::default(), 1.0 / 60.0);
        }
        assert!(b.pos.y >= 1.85 + 0.35 - 1e-3, "{:?}", b.pos);
    }

    #[test]
    fn ray_finds_the_nearest_box() {
        let map = engine_room("");
        let hit = ray_prop(&map, Vec2::new(16.0, 3.0), Vec2::new(0.0, -1.0), 1.6).unwrap();
        assert_eq!(hit.1, 2);
        assert!((hit.0 - 1.15).abs() < 1e-4);
        assert_eq!(
            ray_prop(&map, Vec2::new(16.0, 3.0), Vec2::new(0.0, 1.0), 1.6),
            None
        );
    }

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
