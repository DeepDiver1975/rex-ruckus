//! Player identity and the combat body layout.
//!
//! Combat keeps every body in one slice, players first: `[player 0, .., player n-1, actor 0,
//! actor 1, ..]`. The helpers here are the only place that knows the layout, so the rest of the
//! core never does index arithmetic on body indices itself.

use crate::geom::closest_point_on_segment;
use crate::map::{Map, SectorId};
use crate::movement::Tuning;
use crate::projectile::Shooter;
use crate::trace::can_see;
use glam::Vec2;

/// One player in the game, 0-based (`PlayerId(0)` is the local or host player).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PlayerId(pub u8);

/// The most players one game holds.
pub const MAX_PLAYERS: usize = 4;

/// Body index of actor `i` in `[players.., actors..]`.
pub fn actor_body(players: usize, i: usize) -> usize {
    players + i
}

/// What a body index is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyRef {
    Player(PlayerId),
    /// Index into the actor list.
    Actor(usize),
}

/// What body index `b` is in `[players.., actors..]`.
pub fn body_ref(players: usize, b: usize) -> BodyRef {
    if b < players {
        BodyRef::Player(PlayerId(b as u8))
    } else {
        BodyRef::Actor(b - players)
    }
}

/// The body index of a shooter, if it has one (`World` has none).
pub fn shooter_body(players: usize, s: Shooter) -> Option<usize> {
    match s {
        Shooter::Player(p) => {
            debug_assert!((p.0 as usize) < players, "{p:?} with {players} players");
            Some(p.0 as usize)
        }
        Shooter::Actor(a) => Some(actor_body(players, a)),
        Shooter::World => None,
    }
}

/// Distance (m) between the start and the first ring of co-op spawn spots.
pub const COOP_GAP: f32 = 1.0;
/// Rings of candidate spots tried before falling back on the start.
const COOP_RINGS: usize = 3;
/// Sight lines between spawn spots run at knee height.
const KNEE: f32 = 0.5;

/// The sector a body of `radius` and `height` can stand in at `at` (`hint` speeds up the
/// lookup): a still, hazard-free sector tall enough, clear of every blocking wall.
pub fn fits(
    map: &Map,
    at: Vec2,
    hint: Option<SectorId>,
    radius: f32,
    height: f32,
) -> Option<SectorId> {
    let s = map.find_sector(at, hint)?;
    let sec = &map.sectors[s];
    if sec.mover.is_some() || sec.hazard.is_some() || sec.ceil_z - sec.floor_z < height {
        return None;
    }
    let near_wall = map.walls.iter().any(|w| {
        w.passage().is_none() && closest_point_on_segment(at, w.a, w.b).distance(at) < radius
    });
    (!near_wall).then_some(s)
}

/// Where co-op player `id` starts: id 0 on `start`; the others on the first free candidate
/// around it (relative to `angle`): right, left, behind and ahead at [`COOP_GAP`], then the same
/// at two and three gaps. A candidate is free if a body of `radius`/`height` [`fits`] there,
/// within a step of the start's floor, clear of the spots of every lower id, and the start can
/// see it at knee height. Last resort: `start`.
pub fn coop_spot(
    map: &Map,
    start: Vec2,
    angle: f32,
    id: PlayerId,
    radius: f32,
    height: f32,
) -> Vec2 {
    let Some(sector) = map.find_sector(start, None) else {
        return start;
    };
    let floor = map.sectors[sector].floor_z;
    let forward = Vec2::from_angle(angle);
    let right = Vec2::new(forward.y, -forward.x);
    let candidates: Vec<Vec2> = (1..=COOP_RINGS)
        .flat_map(|ring| {
            let d = COOP_GAP * ring as f32;
            [right, -right, -forward, forward].map(|dir| start + dir * d)
        })
        .collect();
    let eye = start.extend(floor + KNEE);
    let free = |at: Vec2, taken: &[Vec2]| {
        fits(map, at, Some(sector), radius, height).is_some_and(|s| {
            let z = map.sectors[s].floor_z;
            (z - floor).abs() <= Tuning::default().step_height
                && taken.iter().all(|t| t.distance(at) >= 2.0 * radius)
                && can_see(map, eye, sector, at.extend(z + KNEE))
        })
    };
    let mut spots = vec![start];
    for _ in 0..id.0 {
        let next = candidates
            .iter()
            .copied()
            .find(|&c| free(c, &spots))
            .unwrap_or(start);
        spots.push(next);
    }
    spots[id.0 as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::two_rooms;

    const R: f32 = 0.35;
    const H: f32 = 1.8;

    #[test]
    fn coop_spot_spreads_players() {
        let map = two_rooms(0.0, 3.0);
        let start = Vec2::new(2.0, 2.0);
        let spots: Vec<Vec2> = (0..MAX_PLAYERS as u8)
            .map(|i| coop_spot(&map, start, 0.0, PlayerId(i), R, H))
            .collect();
        assert_eq!(spots[0], start);
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!(a.distance(*b) >= 2.0 * R, "{spots:?}");
            }
        }
        // Right of an east-facing start is south.
        assert!(spots[1].y < start.y && (spots[1].x - start.x).abs() < 1e-5);
        assert!(spots[2].y > start.y);
        assert!(spots[3].x < start.x);
    }

    /// A 1.2 m wide corridor along x: x∈[0,10], y∈[0,1.2], start (5,0.6) facing east.
    fn corridor() -> Map {
        Map::from_ron(
            r#"(
            name: "corridor",
            materials: ["wall", "floor", "ceiling"],
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 1.2), (0.0, 1.2)],
            sectors: [
                (loops: [[0, 1, 2, 3]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            ],
            player_start: (pos: (5.0, 0.6), angle_deg: 0.0),
        )"#,
        )
        .unwrap()
    }

    #[test]
    fn coop_spot_falls_back_when_blocked() {
        let map = corridor();
        let start = Vec2::new(5.0, 0.6);
        let spots: Vec<Vec2> = (0..MAX_PLAYERS as u8)
            .map(|i| coop_spot(&map, start, 0.0, PlayerId(i), R, H))
            .collect();
        for s in &spots[1..] {
            assert!(fits(&map, *s, None, R, H).is_some(), "{s} is in a wall");
        }
        assert!(spots[3].x < start.x, "id 3 behind: {spots:?}");
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!(a.distance(*b) >= 2.0 * R - 1e-3, "overlap: {spots:?}");
            }
        }
    }

    #[test]
    fn actor_bodies_follow_the_players() {
        assert_eq!(actor_body(2, 0), 2);
        assert_eq!(actor_body(1, 3), 4);
    }

    #[test]
    fn body_ref_splits_players_from_actors() {
        assert_eq!(body_ref(2, 1), BodyRef::Player(PlayerId(1)));
        assert_eq!(body_ref(2, 3), BodyRef::Actor(1));
        assert_eq!(body_ref(1, 0), BodyRef::Player(PlayerId(0)));
        assert_eq!(body_ref(1, 1), BodyRef::Actor(0));
    }

    #[test]
    fn shooter_body_maps_through_the_layout() {
        assert_eq!(shooter_body(2, Shooter::Actor(0)), Some(2));
        assert_eq!(shooter_body(2, Shooter::Player(PlayerId(1))), Some(1));
        assert_eq!(shooter_body(1, Shooter::World), None);
    }
}
