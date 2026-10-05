//! Hitscan: rays through the sector graph (Build's `hitscan`).

use crate::map::{Map, SectorId, Wall, WallId};
use glam::{Vec2, Vec3};

/// A ray starting at `origin` inside `sector`, travelling along the unit vector `dir` for at most
/// `max` metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    /// Unit length.
    pub dir: Vec3,
    pub sector: SectorId,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// A solid wall, or a portal whose opening the ray misses (step face, soffit, closed door).
    Wall(WallId),
    Floor(SectorId),
    Ceiling(SectorId),
    /// Index into the bodies slice passed to `trace`.
    Body(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub kind: HitKind,
    pub point: Vec3,
    /// Unit surface normal facing back toward the shooter.
    pub normal: Vec3,
    /// Distance along the ray (the ray parameter `t`).
    pub dist: f32,
    /// Sector the ray was in when it hit.
    pub sector: SectorId,
}

/// The first world surface (wall, floor or ceiling) the ray meets within `ray.max`.
///
/// The walk starts in `ray.sector` (the origin must be inside it) and hops through portals. At
/// a portal into sector `n` the ray carries on only if it crosses strictly inside `n`'s
/// floor-to-ceiling opening; otherwise the portal wall itself is hit (step faces, soffits and
/// closed doors). `None` means nothing within `ray.max`, and is also returned if the walk
/// exceeds its hop cap (`4 × sectors + 16`), which only a numerically degenerate map can cause.
pub fn trace_world(map: &Map, ray: &Ray) -> Option<Hit> {
    debug_assert!(
        (ray.dir.length() - 1.0).abs() < 1e-3,
        "ray.dir must be unit length, got {:?}",
        ray.dir
    );
    let (o, d) = (ray.origin, ray.dir);
    let (o2, d2) = (o.truncate(), d.truncate());
    let at = |t: f32| o + d * t;
    let mut sector = ray.sector;
    let mut t_enter = 0.0_f32;

    for _ in 0..4 * map.sectors.len() + 16 {
        let sec = &map.sectors[sector];

        // Nearest front-facing (exiting) wall crossing at or after the entry point.
        let exit = sec
            .walls()
            .filter_map(|wid| {
                let (t, u) = wall_t(o2, d2, &map.walls[wid])?;
                (t >= t_enter - T_EPS && (-U_EPS..=1.0 + U_EPS).contains(&u)).then_some((t, wid))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));

        // Floor or ceiling plane, whichever the ray is heading for.
        let plane = if d.z < 0.0 {
            Some((
                ((sec.floor_z - o.z) / d.z).max(t_enter),
                HitKind::Floor(sector),
                Vec3::Z,
            ))
        } else if d.z > 0.0 {
            Some((
                ((sec.ceil_z - o.z) / d.z).max(t_enter),
                HitKind::Ceiling(sector),
                Vec3::NEG_Z,
            ))
        } else {
            None
        };

        if let Some((t, kind, normal)) = plane
            && exit.is_none_or(|(tw, _)| t < tw)
        {
            return (t <= ray.max).then(|| Hit {
                kind,
                point: at(t),
                normal,
                dist: t,
                sector,
            });
        }

        let (t, wid) = exit?;
        if t > ray.max {
            return None;
        }
        let wall = &map.walls[wid];
        let p = at(t);
        if let Some(n) = wall.next_sector {
            let next = &map.sectors[n];
            if next.floor_z < p.z && p.z < next.ceil_z {
                sector = n;
                t_enter = t;
                continue;
            }
        }
        return Some(Hit {
            kind: HitKind::Wall(wid),
            point: p,
            normal: wall.inward_normal().extend(0.0),
            dist: t,
            sector,
        });
    }
    None
}

/// Slack on the entry parameter, so a crossing exactly at the portal just entered (a ray through
/// a shared vertex) still counts as the way out.
const T_EPS: f32 = 1e-5;
/// Slack on the position along a wall, so a ray through a shared vertex cannot slip between the
/// two walls meeting there through rounding.
const U_EPS: f32 = 1e-5;

/// Where the 2D ray `o + t·d` crosses the line through wall `w`, counting only crossings of the
/// wall's front side (the side facing into its own sector, i.e. a ray leaving that sector).
/// Returns `(t, u)`, where `u` is the position along a→b (0 at `a`, 1 at `b`). Neither `t` nor
/// `u` is range-checked; callers decide what counts.
pub(crate) fn wall_t(o: Vec2, d: Vec2, w: &Wall) -> Option<(f32, f32)> {
    if d.dot(w.inward_normal()) >= 0.0 {
        return None;
    }
    let e = w.b - w.a;
    let denom = d.perp_dot(e);
    if denom.abs() < 1e-9 {
        return None;
    }
    let ao = w.a - o;
    Some((ao.perp_dot(e) / denom, ao.perp_dot(d) / denom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::combat_room;
    use crate::mechanics::Mechanics;
    use crate::validate::{Severity, validate};

    const EPS: f32 = 1e-4;

    fn ray(map: &Map, origin: Vec3, dir: Vec3, max: f32) -> Ray {
        let sector = map
            .find_sector(origin.truncate(), None)
            .expect("origin inside map");
        Ray {
            origin,
            dir: dir.normalize(),
            sector,
            max,
        }
    }

    fn shoot(map: &Map, origin: Vec3, dir: Vec3) -> Hit {
        trace_world(map, &ray(map, origin, dir, 200.0)).expect("ray hits something")
    }

    fn wall_hit(h: &Hit) -> WallId {
        match h.kind {
            HitKind::Wall(w) => w,
            k => panic!("expected a wall hit, got {k:?} at {:?}", h.point),
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < EPS
    }

    fn closed(mut map: Map) -> Map {
        Mechanics::new(&mut map);
        map
    }

    #[test]
    fn combat_room_validates_without_errors() {
        let map = combat_room();
        let errors: Vec<_> = validate(&map)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.sectors.len(), 5);
    }

    #[test]
    fn shot_stops_at_closed_door() {
        let map = closed(combat_room());
        assert_eq!(map.sectors[3].ceil_z, map.sectors[3].floor_z);
        let h = shoot(&map, Vec3::new(6.0, 4.0, 1.5), Vec3::X);
        let w = wall_hit(&h);
        assert_eq!(map.walls[w].sector, 0);
        assert_eq!(map.walls[w].next_sector, Some(3), "the door's face");
        assert!(close(h.point, Vec3::new(12.0, 4.0, 1.5)), "{h:?}");
        assert!((h.dist - 6.0).abs() < EPS);
        assert_eq!(h.sector, 0);
    }

    #[test]
    fn shot_passes_open_door() {
        let map = combat_room();
        let h = shoot(&map, Vec3::new(6.0, 4.0, 1.5), Vec3::X);
        let w = wall_hit(&h);
        assert_eq!(map.walls[w].sector, 4);
        assert_eq!(map.walls[w].next_sector, None);
        assert!(close(h.point, Vec3::new(16.5, 4.0, 1.5)), "{h:?}");
        assert!((h.dist - 10.5).abs() < EPS);
        assert_eq!(h.sector, 4);
    }

    #[test]
    fn low_shot_hits_step_face() {
        let map = combat_room();
        let h = shoot(&map, Vec3::new(6.0, 1.5, 0.25), Vec3::X);
        let w = wall_hit(&h);
        assert_eq!(
            (map.walls[w].sector, map.walls[w].next_sector),
            (0, Some(1))
        );
        assert!(close(h.point, Vec3::new(8.0, 1.5, 0.25)), "{h:?}");
        // At eye height the same line clears the step and stops at the step's far wall.
        let h = shoot(&map, Vec3::new(6.0, 1.5, 1.5), Vec3::X);
        assert!(close(h.point, Vec3::new(12.0, 1.5, 1.5)), "{h:?}");
        assert_eq!(h.sector, 1);
    }

    #[test]
    fn high_shot_hits_upper_face() {
        let map = combat_room();
        let h = shoot(&map, Vec3::new(6.0, 6.5, 3.0), Vec3::X);
        let w = wall_hit(&h);
        assert_eq!(
            (map.walls[w].sector, map.walls[w].next_sector),
            (0, Some(2))
        );
        assert!(close(h.point, Vec3::new(8.0, 6.5, 3.0)), "{h:?}");
        // Below the soffit the shot passes into it.
        let h = shoot(&map, Vec3::new(6.0, 6.5, 1.5), Vec3::X);
        assert!(close(h.point, Vec3::new(12.0, 6.5, 1.5)), "{h:?}");
        assert_eq!(h.sector, 2);
    }

    #[test]
    fn downward_shot_hits_floor_in_far_sector() {
        let map = combat_room();
        // Drops 0.2 m per metre: 1.2 m high at the step edge (x=8), meets its 0.5 m floor at x=11.5.
        let h = shoot(&map, Vec3::new(6.0, 1.5, 1.6), Vec3::new(1.0, 0.0, -0.2));
        assert_eq!(h.kind, HitKind::Floor(1));
        assert_eq!(h.sector, 1);
        assert!(close(h.point, Vec3::new(11.5, 1.5, 0.5)), "{h:?}");
        assert_eq!(h.normal, Vec3::Z);
        assert!((h.dist - Vec3::new(5.5, 0.0, -1.1).length()).abs() < EPS);
    }

    #[test]
    fn upward_shot_hits_ceiling() {
        let map = combat_room();
        let h = shoot(&map, Vec3::new(1.5, 1.5, 1.5), Vec3::new(1.0, 0.0, 1.0));
        assert_eq!(h.kind, HitKind::Ceiling(0));
        assert!(close(h.point, Vec3::new(4.0, 1.5, 4.0)), "{h:?}");
        assert_eq!(h.normal, -Vec3::Z);
    }

    #[test]
    fn shot_hits_pillar_hole_wall() {
        let map = combat_room();
        let h = shoot(&map, Vec3::new(1.5, 4.0, 1.5), Vec3::X);
        let w = wall_hit(&h);
        let wall = &map.walls[w];
        assert_eq!(wall.next_sector, None);
        assert!(map.sectors[0].loops[1].contains(&w), "a hole-loop wall");
        assert!(close(h.point, Vec3::new(3.0, 4.0, 1.5)), "{h:?}");
        assert!((h.dist - 1.5).abs() < EPS);
    }

    #[test]
    fn ray_out_of_range_is_none() {
        let map = combat_room();
        let o = Vec3::new(1.5, 4.0, 1.5);
        assert_eq!(trace_world(&map, &ray(&map, o, Vec3::X, 1.0)), None);
        assert!(trace_world(&map, &ray(&map, o, Vec3::X, 2.0)).is_some());
        // Out of range through a portal too.
        let o = Vec3::new(6.0, 4.0, 1.5);
        assert_eq!(trace_world(&map, &ray(&map, o, Vec3::X, 10.0)), None);
    }

    #[test]
    fn ray_through_shared_vertex_continues() {
        let map = combat_room();
        // From the step through vertex (8,3) into the main room, out to the north wall at (3,8).
        let h = shoot(&map, Vec3::new(10.0, 1.0, 1.5), Vec3::new(-1.0, 1.0, 0.0));
        assert_eq!(h.sector, 0);
        wall_hit(&h);
        assert!(close(h.point, Vec3::new(3.0, 8.0, 1.5)), "{h:?}");
        // And back the other way, onto the step's south wall at (11,0).
        let h = shoot(&map, Vec3::new(4.0, 7.0, 1.5), Vec3::new(1.0, -1.0, 0.0));
        assert_eq!(h.sector, 1);
        wall_hit(&h);
        assert!(close(h.point, Vec3::new(11.0, 0.0, 1.5)), "{h:?}");
    }

    #[test]
    fn normals_face_the_shooter() {
        let map = closed(combat_room());
        let shots = [
            (Vec3::new(6.0, 4.0, 1.5), Vec3::X),  // closed door
            (Vec3::new(6.0, 1.5, 0.25), Vec3::X), // step face
            (Vec3::new(6.0, 6.5, 3.0), Vec3::X),  // soffit face
            (Vec3::new(1.5, 4.0, 1.5), Vec3::X),  // pillar
            (Vec3::new(6.0, 1.5, 1.6), Vec3::new(1.0, 0.0, -0.2)), // floor
            (Vec3::new(1.5, 1.5, 1.5), Vec3::new(1.0, 0.0, 1.0)), // ceiling
            (Vec3::new(2.0, 2.0, 1.5), Vec3::new(-1.0, -0.5, 0.0)), // outer wall corner region
        ];
        for (o, d) in shots {
            let h = shoot(&map, o, d);
            assert!((h.normal.length() - 1.0).abs() < EPS, "{h:?}");
            assert!(
                h.normal.dot(d.normalize()) < 0.0,
                "normal faces away: {h:?}"
            );
            assert!((h.point - (o + d.normalize() * h.dist)).length() < EPS);
        }
    }
}
