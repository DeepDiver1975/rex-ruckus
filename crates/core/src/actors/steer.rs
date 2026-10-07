//! Corner steering: when the straight line to a goal is blocked by a pillar or an inside corner
//! of the actor's own sector, aim at an offset corner vertex instead of sliding along the wall.
//! `path_open` says whether a straight line across sectors is passable for a mover at all.

use crate::map::{Map, SectorId};
use crate::movement::Pass;
use glam::Vec2;

/// Extra standoff beyond the body radius when a corner is offset outward.
const CORNER_MARGIN: f32 = 0.15;
/// Corner legs are judged with this fraction of the radius: a body that has just reached a corner
/// sits a little inside its offset.
pub const CORNER_SLACK: f32 = 0.8;

fn seg_point_dist(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let d = b - a;
    let l2 = d.length_squared();
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + d * t)
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn segs_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (d1, d2) = (cross(b - a, c - a), cross(b - a, d - a));
    let (d3, d4) = (cross(d - c, a - c), cross(d - c, b - c));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Smallest distance between segments `ab` and `cd` (zero if they cross).
fn seg_seg_dist(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f32 {
    if segs_cross(a, b, c, d) {
        return 0.0;
    }
    seg_point_dist(a, b, c)
        .min(seg_point_dist(a, b, d))
        .min(seg_point_dist(c, d, a))
        .min(seg_point_dist(c, d, b))
}

/// Whether a body of `radius` can walk the straight segment `from → to` without touching a solid
/// wall of `sector` (outer loop and holes; portals are open). A wall lying within `radius` of
/// the goal and not crossed by the line only borders it, so it does not count: a goal hugging a
/// wall stays reachable.
pub fn line_clear(map: &Map, sector: SectorId, from: Vec2, to: Vec2, radius: f32) -> bool {
    map.sectors[sector]
        .walls()
        .map(|w| &map.walls[w])
        .filter(|w| w.passage().is_none())
        .all(|w| {
            let bordering =
                seg_point_dist(w.a, w.b, to) < radius && !segs_cross(from, to, w.a, w.b);
            bordering || seg_seg_dist(from, to, w.a, w.b) >= radius
        })
}

/// The obstruction corners of `sector`, offset into free space so a body of `radius` fits.
fn corners(map: &Map, sector: SectorId, radius: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    for lp in &map.sectors[sector].loops {
        for (i, &w0) in lp.iter().enumerate() {
            let (w0, w1) = (&map.walls[w0], &map.walls[lp[(i + 1) % lp.len()]]);
            // Hole vertices and reflex outer vertices both turn right (outer loops run
            // counter-clockwise, holes clockwise). Corners of two open portals obstruct nothing.
            let turn = cross(
                (w0.b - w0.a).normalize_or_zero(),
                (w1.b - w1.a).normalize_or_zero(),
            );
            if turn >= 0.0 || (w0.passage().is_some() && w1.passage().is_some()) {
                continue;
            }
            let s = w0.inward_normal() + w1.inward_normal();
            let len = s.length();
            if len < 1e-4 {
                continue;
            }
            // Distance along the bisector at which the point is `radius + margin` off both walls.
            let off = (radius + CORNER_MARGIN) / (len / 2.0).max(0.3);
            out.push(w0.b + s / len * off);
        }
    }
    out
}

/// A corner of `sector` to steer to when the straight line `from → goal` is blocked: the visible
/// offset corner minimising `|from→corner| + |corner→goal|`. `None` if the line is clear or no
/// corner is visible.
pub fn steer_corner(
    map: &Map,
    sector: SectorId,
    from: Vec2,
    goal: Vec2,
    radius: f32,
) -> Option<Vec2> {
    if line_clear(map, sector, from, goal, radius) {
        return None;
    }
    corners(map, sector, radius)
        .into_iter()
        .filter(|&c| {
            c.distance(from) > 0.1 && line_clear(map, sector, from, c, radius * CORNER_SLACK)
        })
        .min_by(|&x, &y| {
            let cost = |c: Vec2| from.distance(c) + c.distance(goal);
            cost(x).total_cmp(&cost(y))
        })
}

/// Where to head to reach `goal`: the goal itself when the line is clear (or no corner helps),
/// otherwise the best corner (see `steer_corner`).
pub fn steer_target(map: &Map, sector: SectorId, from: Vec2, goal: Vec2, radius: f32) -> Vec2 {
    steer_corner(map, sector, from, goal, radius).unwrap_or(goal)
}

/// Whether a mover with `pass` can follow the straight segment `from → to` (started in `sector`)
/// through every sector it crosses: false at the first solid wall (intact glass included) the
/// segment hits, or at the first portal `pass` cannot take on live floor and ceiling heights.
/// The body's radius is not considered; that is `line_clear`'s job within a sector.
pub fn path_open(map: &Map, sector: SectorId, from: Vec2, to: Vec2, pass: Pass) -> bool {
    let pose = |s: SectorId| (map.sectors[s].floor_z, map.sectors[s].ceil_z);
    let d = to - from;
    let (mut s, mut t0) = (sector, 0.0_f32);
    // Each step crosses a different wall at a larger `t`, so the walls bound the walk.
    for _ in 0..=map.walls.len() {
        // The first wall of `s` the segment crosses beyond the point where it entered `s`.
        let exit = map.sectors[s]
            .walls()
            .map(|w| &map.walls[w])
            .filter(|w| segs_cross(from, to, w.a, w.b))
            .map(|w| (cross(w.a - from, w.b - w.a) / cross(d, w.b - w.a), w))
            .filter(|&(t, _)| t > t0 + 1e-6)
            .min_by(|x, y| x.0.total_cmp(&y.0));
        let Some((t, w)) = exit else {
            return true; // `to` lies in `s`
        };
        match w.passage() {
            Some(n) if pass.allows(pose(s), pose(n)) => (s, t0) = (n, t),
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{glass_rooms, hall_and_tunnel, pillar_room};

    const R: f32 = 0.35;

    #[test]
    fn clear_line_returns_the_goal() {
        let map = pillar_room();
        let (from, goal) = (Vec2::new(1.0, 1.0), Vec2::new(9.0, 1.0));
        assert!(line_clear(&map, 0, from, goal, R));
        assert_eq!(steer_target(&map, 0, from, goal, R), goal);
    }

    #[test]
    fn pillar_blocks_and_a_corner_is_chosen() {
        let map = pillar_room();
        let (from, goal) = (Vec2::new(2.0, 5.0), Vec2::new(8.0, 5.0));
        assert!(!line_clear(&map, 0, from, goal, R));
        let c = steer_target(&map, 0, from, goal, R);
        assert_ne!(c, goal);
        // Outside the pillar by at least the body radius, and visible.
        let pillar_d = (c.x.clamp(4.0, 6.0) - c.x).hypot(c.y.clamp(4.0, 6.0) - c.y);
        assert!(pillar_d >= R, "corner {c} too close: {pillar_d}");
        assert!(line_clear(&map, 0, from, c, R));
    }

    #[test]
    fn near_miss_of_the_pillar_edge_is_still_blocked() {
        let map = pillar_room();
        // Passes 0.2 m below the pillar's south face: inside the body radius.
        let (from, goal) = (Vec2::new(2.0, 3.8), Vec2::new(8.0, 3.8));
        assert!(!line_clear(&map, 0, from, goal, R));
        assert!(line_clear(&map, 0, from, Vec2::new(8.0, 3.0), R));
    }

    #[test]
    fn goal_hugging_a_wall_stays_reachable() {
        let map = pillar_room();
        assert!(line_clear(
            &map,
            0,
            Vec2::new(2.0, 1.0),
            Vec2::new(8.0, 0.1),
            R
        ));
    }

    #[test]
    fn corners_cover_holes_but_not_convex_outer_vertices() {
        let map = pillar_room();
        let cs = corners(&map, 0, R);
        assert_eq!(cs.len(), 4, "{cs:?}");
        for c in cs {
            assert!(c.x < 4.0 || c.x > 6.0);
            assert!(c.y < 4.0 || c.y > 6.0);
        }
    }

    #[test]
    fn no_visible_corner_falls_back_to_the_goal() {
        let map = pillar_room();
        // Inside the pillar footprint nothing is visible; the old behaviour is kept.
        let (from, goal) = (Vec2::new(5.0, 5.0), Vec2::new(9.0, 5.0));
        let c = steer_corner(&map, 0, from, goal, R);
        assert!(c.is_none() || c.is_some_and(|c| line_clear(&map, 0, from, c, R)));
        assert_eq!(steer_target(&map, 0, from, goal, 50.0), goal);
    }

    #[test]
    fn path_open_respects_the_movers_height() {
        let map = hall_and_tunnel();
        let boss = Pass::Fly { height: 3.0 };
        let short = Pass::Fly { height: 0.6 };
        let (from, to) = (Vec2::new(4.0, 5.0), Vec2::new(16.0, 5.0));
        assert!(!path_open(&map, 0, from, to, boss));
        assert!(path_open(&map, 0, from, to, short));
        assert!(path_open(&map, 0, from, Vec2::new(8.0, 2.0), boss)); // same sector
        // A walker carries its height as the crouch height.
        let d = crate::fixtures::defs();
        let walk = |k| Pass::Walk(d.enemy(k).tuning());
        assert!(!path_open(
            &map,
            0,
            from,
            to,
            walk(crate::map::ActorKind::Boss)
        ));
        assert!(path_open(
            &map,
            0,
            from,
            to,
            walk(crate::map::ActorKind::Grunt)
        ));
        // Back out of the tunnel too.
        assert!(!path_open(&map, 1, to, from, boss));
        assert!(path_open(&map, 1, to, from, short));
    }

    #[test]
    fn path_open_is_blocked_by_solid_walls_and_glass() {
        let map = hall_and_tunnel();
        let short = Pass::Fly { height: 0.6 };
        // Through the hall's east wall beside the tunnel mouth.
        assert!(!path_open(
            &map,
            0,
            Vec2::new(4.0, 9.0),
            Vec2::new(16.0, 5.0),
            short
        ));
        let map = glass_rooms();
        // Through the intact pane at x = 10.
        assert!(!path_open(
            &map,
            0,
            Vec2::new(8.0, 5.0),
            Vec2::new(12.0, 5.0),
            short
        ));
        // Through the pillar hole.
        assert!(!path_open(
            &map,
            0,
            Vec2::new(2.0, 5.0),
            Vec2::new(8.0, 5.0),
            short
        ));
    }

    #[test]
    fn path_open_through_broken_glass() {
        let mut map = glass_rooms();
        for w in &mut map.walls {
            w.glass = false;
        }
        let short = Pass::Fly { height: 0.6 };
        assert!(path_open(
            &map,
            0,
            Vec2::new(8.0, 5.0),
            Vec2::new(12.0, 5.0),
            short
        ));
    }
}
