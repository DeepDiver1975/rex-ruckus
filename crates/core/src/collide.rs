//! Build-style collision: a vertical cylinder (circle in XY + height) slides along walls.
//! Portals block when the sector beyond is too high to step onto or too low to fit in.

use crate::geom::closest_point_on_segment;
use crate::map::{Map, Sector, SectorId, Wall};
use glam::{Vec2, Vec3};

const SKIN: f32 = 1e-3;
/// Overlap needed before a neighbouring sector's floor/ceiling affects `z_range`.
const Z_RANGE_MARGIN: f32 = 0.01;
const PUSH_ITERATIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// Feet position in core coordinates.
    pub pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub height: f32,
    pub sector: SectorId,
    pub on_ground: bool,
}

impl Body {
    /// Places a body with its feet on the floor of the sector containing `xy`.
    pub fn spawn(map: &Map, xy: Vec2, radius: f32, height: f32) -> Option<Body> {
        let sector = map.find_sector(xy, None)?;
        Some(Body {
            pos: xy.extend(map.sectors[sector].floor_z),
            vel: Vec3::ZERO,
            radius,
            height,
            sector,
            on_ground: true,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipResult {
    pub blocked: bool,
}

fn can_hold(s: &Sector, feet: f32, height: f32, step: f32) -> bool {
    s.floor_z <= feet + step + SKIN && s.ceil_z - s.floor_z.max(feet) >= height - SKIN
}

/// A portal only needs to admit the sector the body is moving into, so a body
/// standing in a sector it cannot hold can still leave it.
fn wall_blocks(map: &Map, w: &Wall, body: &Body, step: f32) -> bool {
    let (feet, height) = (body.pos.z, body.height);
    match w.next_sector {
        None => true,
        Some(t) => {
            let near = !can_hold(&map.sectors[w.sector], feet, height, step);
            let far = !can_hold(&map.sectors[t], feet, height, step);
            if body.sector == w.sector {
                far
            } else if body.sector == t {
                near
            } else {
                near || far
            }
        }
    }
}

/// Sectors reachable from `start` through portals passing within `reach` of `p`.
fn nearby_sectors(map: &Map, start: SectorId, p: Vec2, reach: f32) -> Vec<SectorId> {
    let mut found = vec![start];
    let mut i = 0;
    while i < found.len() {
        let s = found[i];
        i += 1;
        for wid in map.sectors[s].walls() {
            let w = &map.walls[wid];
            if let Some(t) = w.next_sector
                && !found.contains(&t)
                && closest_point_on_segment(p, w.a, w.b).distance(p) < reach
            {
                found.push(t);
            }
        }
    }
    found
}

/// Moves `body` by `delta` in XY, sliding along blocking walls. Movement is split into
/// sub-steps of at most half the radius, so fast bodies cannot tunnel through walls.
///
/// Feet z is held constant for the whole call: callers move once per tick and update z
/// between calls.
pub fn clip_move(map: &Map, body: &mut Body, delta: Vec2, step_height: f32) -> ClipResult {
    let len = delta.length();
    if len <= f32::EPSILON {
        return ClipResult::default();
    }
    let n = ((len / (body.radius * 0.5)).ceil() as usize).clamp(1, 4096);
    let sub = delta / n as f32;
    let mut blocked = false;

    for _ in 0..n {
        let mut p = body.pos.truncate() + sub;
        for _ in 0..PUSH_ITERATIONS {
            let mut pushed = false;
            for s in nearby_sectors(map, body.sector, p, body.radius + sub.length() + SKIN) {
                for wid in map.sectors[s].walls() {
                    let w = &map.walls[wid];
                    if !wall_blocks(map, w, body, step_height) {
                        continue;
                    }
                    let c = closest_point_on_segment(p, w.a, w.b);
                    let v = p - c;
                    let d = v.length();
                    if d < body.radius - SKIN {
                        let normal = if d > 1e-6 { v / d } else { w.inward_normal() };
                        p = c + normal * body.radius;
                        pushed = true;
                        blocked = true;
                    }
                }
            }
            if !pushed {
                break;
            }
        }
        match map.find_sector(p, Some(body.sector)) {
            Some(s)
                if s == body.sector
                    || can_hold(&map.sectors[s], body.pos.z, body.height, step_height) =>
            {
                body.pos.x = p.x;
                body.pos.y = p.y;
                body.sector = s;
            }
            _ => return ClipResult { blocked: true },
        }
    }
    ClipResult { blocked }
}

/// Highest floor and lowest ceiling under a circle (like Build's `getzrange`).
pub fn z_range(map: &Map, p: Vec2, radius: f32, sector: SectorId) -> (f32, f32) {
    nearby_sectors(map, sector, p, radius - Z_RANGE_MARGIN)
        .into_iter()
        .map(|s| &map.sectors[s])
        .fold((f32::MIN, f32::MAX), |(f, c), s| {
            (f.max(s.floor_z), c.min(s.ceil_z))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{pillar_room, two_rooms};

    const R: f32 = 0.35;
    const H: f32 = 1.8;
    const STEP: f32 = 0.55;

    fn body(map: &Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), R, H).unwrap()
    }

    #[test]
    fn spawn_sits_on_floor() {
        let map = two_rooms(1.0, 3.0);
        let b = body(&map, 6.0, 2.0);
        assert_eq!((b.sector, b.pos.z, b.on_ground), (1, 1.0, true));
    }

    #[test]
    fn spawn_outside_returns_none() {
        assert!(Body::spawn(&two_rooms(0.0, 3.0), Vec2::new(-5.0, 0.0), R, H).is_none());
    }

    #[test]
    fn solid_wall_stops_movement() {
        let map = two_rooms(2.0, 3.0); // B too high to step onto: the portal acts as a wall
        let mut b = body(&map, 2.0, 2.0);
        let r = clip_move(&map, &mut b, Vec2::new(10.0, 0.0), STEP);
        assert!(r.blocked);
        assert_eq!(b.sector, 0);
        assert!(b.pos.x <= 4.0 - R + 1e-3, "x = {}", b.pos.x);
    }

    #[test]
    fn sliding_keeps_tangential_motion() {
        let map = two_rooms(0.0, 3.0);
        let mut b = body(&map, 1.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(-5.0, 1.0), STEP);
        assert!((b.pos.x - R).abs() < 1e-3, "x = {}", b.pos.x);
        assert!(b.pos.y > 2.9, "y = {}", b.pos.y);
    }

    #[test]
    fn low_step_can_be_climbed() {
        let map = two_rooms(0.4, 3.0);
        let mut b = body(&map, 2.0, 2.0);
        let r = clip_move(&map, &mut b, Vec2::new(4.0, 0.0), STEP);
        assert!(!r.blocked);
        assert_eq!(b.sector, 1);
    }

    #[test]
    fn low_ceiling_blocks_standing_body() {
        let map = two_rooms(0.0, 1.2);
        let mut b = body(&map, 2.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(4.0, 0.0), STEP);
        assert_eq!(b.sector, 0);
        assert!(b.pos.x <= 4.0 - R + 1e-3, "x = {}", b.pos.x);
    }

    #[test]
    fn cannot_drop_into_sector_with_too_low_opening() {
        let map = two_rooms(2.0, 5.0); // A ceil 3, B floor 2: only 1 m of headroom in A
        let mut b = body(&map, 6.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(-4.0, 0.0), STEP);
        assert_eq!(b.sector, 1);
        assert!(b.pos.x >= 4.0 + R - 1e-3, "x = {}", b.pos.x);
    }

    #[test]
    fn can_leave_a_sector_too_low_to_hold_body() {
        let map = two_rooms(0.0, 1.5); // B is only 1.5 m tall; body is 1.8 m
        let mut b = body(&map, 6.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(-4.0, 0.0), STEP);
        assert_eq!(b.sector, 0);
    }

    #[test]
    fn huge_delta_does_not_tunnel() {
        let map = two_rooms(2.0, 3.0);
        let mut b = body(&map, 2.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(1000.0, 3.0), STEP);
        assert_eq!(b.sector, 0);
        assert!(map.sector_contains(0, b.pos.truncate()));
    }

    #[test]
    fn corner_does_not_leak() {
        let map = pillar_room();
        let mut b = body(&map, 2.0, 2.0);
        for _ in 0..50 {
            clip_move(&map, &mut b, Vec2::new(-0.3, -0.3), STEP); // push into the (0,0) corner
        }
        assert!(b.pos.x >= R - 1e-3 && b.pos.y >= R - 1e-3, "{}", b.pos);
        let mut b = body(&map, 2.0, 5.0);
        clip_move(&map, &mut b, Vec2::new(6.0, 0.0), STEP); // straight into the pillar
        assert!(b.pos.x <= 4.0 - R + 1e-3, "{}", b.pos);
    }

    #[test]
    fn z_range_sees_overlapped_step() {
        let map = two_rooms(0.4, 2.5);
        assert_eq!(z_range(&map, Vec2::new(3.8, 2.0), R, 0), (0.4, 2.5));
        assert_eq!(z_range(&map, Vec2::new(3.5, 2.0), R, 0), (0.0, 3.0));
    }
}
