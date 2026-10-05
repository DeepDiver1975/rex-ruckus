//! Splash damage: which bodies an explosion reaches and for how much.

use crate::collide::Body;
use crate::defs::SplashDef;
use crate::map::{Map, MoverKind, SectorId, WallId};
use crate::projectile::Shooter;
use crate::trace::{Hit, HitKind, Ray, can_see, trace_world};
use glam::Vec3;

/// One explosion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    /// Where it goes off, taken as given. A caller that has a hit normal nudges the point 5 cm
    /// along it first, so the centre is not inside the surface that was hit.
    pub center: Vec3,
    /// Sector containing `center`.
    pub sector: SectorId,
    pub splash: SplashDef,
    pub owner: Shooter,
}

/// Damage dealt to one body (index into the `bodies` slice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlastHit {
    pub body: usize,
    pub damage: i32,
}

/// Distance from `p` to the body's vertical cylinder (0 inside it).
fn distance_to_body(p: Vec3, body: &Body) -> f32 {
    let horiz = (p.truncate().distance(body.pos.truncate()) - body.radius).max(0.0);
    let vert = (p.z.clamp(body.pos.z, body.pos.z + body.height) - p.z).abs();
    horiz.hypot(vert)
}

/// Splash hits in ascending body order. A body is exposed when the world leaves a clear line
/// from the centre to its middle or to its head (walls and closed doors block). Damage falls
/// linearly from `splash.damage` at the cylinder surface to 0 at `splash.radius`; the owner's
/// own body takes `self_scale` of it. `skip` excludes bodies (e.g. the dead).
pub fn solve(
    map: &Map,
    blast: &Blast,
    bodies: &[Body],
    skip: impl Fn(usize) -> bool,
) -> Vec<BlastHit> {
    let s = &blast.splash;
    let owner = match blast.owner {
        Shooter::Player => 0,
        Shooter::Actor(a) => a + 1,
    };
    let mut hits = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        if skip(i) {
            continue;
        }
        let d = distance_to_body(blast.center, body);
        if d >= s.radius {
            continue;
        }
        let mut damage = (s.damage as f32 * (1.0 - d / s.radius)).round();
        if i == owner {
            damage = (damage * s.self_scale).round();
        }
        if damage < 1.0 {
            continue;
        }
        // Chest and head, never feet: see `can_see`.
        let xy = body.pos.truncate();
        let exposed = [0.5, 0.9].iter().any(|f| {
            can_see(
                map,
                blast.center,
                blast.sector,
                xy.extend(body.pos.z + body.height * f),
            )
        });
        if exposed {
            hits.push(BlastHit {
                body: i,
                damage: damage as i32,
            });
        }
    }
    hits
}

/// Glass panes a blast shatters, one wall id per pane (the side with the lower id). A pane is
/// reached when the point of its opening (between the higher floor and the lower ceiling of
/// the two sectors) nearest the centre lies within `splash.radius`, and the world leaves a
/// clear line from the centre to that point, the pane itself being where the line ends.
pub fn walls_in_reach(map: &Map, blast: &Blast) -> Vec<WallId> {
    let c = blast.center;
    let mut out = Vec::new();
    for (id, w) in map.walls.iter().enumerate() {
        let (Some(back), Some(far)) = (w.next_wall, w.next_sector) else {
            continue;
        };
        if !w.glass || back < id {
            continue;
        }
        let (near, far) = (&map.sectors[w.sector], &map.sectors[far]);
        let (lo, hi) = (near.floor_z.max(far.floor_z), near.ceil_z.min(far.ceil_z));
        let edge = w.b - w.a;
        let len = edge.length();
        if hi <= lo || len <= f32::EPSILON {
            continue;
        }
        // The nearest point of the opening, pulled in from its rim so the line from the centre
        // lands on the pane rather than on the frame around it.
        let (du, dz) = (PANE_INSET.min(len * 0.5), PANE_INSET.min((hi - lo) * 0.5));
        let u = ((c.truncate() - w.a).dot(edge) / (len * len)).clamp(du / len, 1.0 - du / len);
        let target = (w.a + edge * u).extend(c.z.clamp(lo + dz, hi - dz));
        if target.distance(c) >= blast.splash.radius {
            continue;
        }
        if pane_in_view(map, blast, target, [id, back]) {
            out.push(id);
        }
    }
    out
}

/// Closed crack walls a blast opens, ascending. A crack is reached when it contains the blast
/// centre, or when the point of one of its portal walls nearest the centre lies within
/// `splash.radius` and the world leaves a clear line from the centre to it. A closed crack has
/// no opening (floor = ceiling), so the portal wall is measured at the crack's floor height
/// (just above, and not below the neighbour's floor, so the line lands on the wall face).
/// Cracks that are already opening or open are not reached again.
pub fn sectors_in_reach(map: &Map, blast: &Blast) -> Vec<SectorId> {
    let c = blast.center;
    let mut out = Vec::new();
    for (s, sec) in map.sectors.iter().enumerate() {
        if sec.mover.is_none_or(|m| m.kind != MoverKind::Crack) || sec.ceil_z > sec.floor_z {
            continue;
        }
        if s == blast.sector {
            out.push(s);
            continue;
        }
        let reached = sec.walls().any(|id| {
            let w = &map.walls[id];
            let (Some(back), Some(far)) = (w.next_wall, w.next_sector) else {
                return false;
            };
            let edge = w.b - w.a;
            let len = edge.length();
            if len <= f32::EPSILON {
                return false;
            }
            let du = PANE_INSET.min(len * 0.5) / len;
            let u = ((c.truncate() - w.a).dot(edge) / (len * len)).clamp(du, 1.0 - du);
            let z = sec.floor_z.max(map.sectors[far].floor_z) + CRACK_AIM;
            let target = (w.a + edge * u).extend(z);
            target.distance(c) < blast.splash.radius && pane_in_view(map, blast, target, [id, back])
        });
        if reached {
            out.push(s);
        }
    }
    out
}

/// How far (m) above the floor `sectors_in_reach` aims at a crack's wall.
const CRACK_AIM: f32 = 0.05;

/// Whether the first thing on the line from the blast centre to `target` (a point on a pane)
/// is one of the pane's two `sides`.
fn pane_in_view(map: &Map, blast: &Blast, target: Vec3, sides: [WallId; 2]) -> bool {
    let delta = target - blast.center;
    let dist = delta.length();
    if dist <= f32::EPSILON {
        return true;
    }
    let ray = Ray {
        origin: blast.center,
        dir: delta / dist,
        sector: blast.sector,
        max: dist + PANE_INSET,
    };
    matches!(
        trace_world(map, &ray),
        Some(Hit { kind: HitKind::Wall(h), .. }) if sides.contains(&h)
    )
}

/// How far (m) inside the rim of a glass opening `walls_in_reach` aims.
const PANE_INSET: f32 = 0.02;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{door_rooms, pillar_room};
    use crate::map::Map;
    use crate::mechanics::Mechanics;
    use glam::Vec2;

    const SPLASH: SplashDef = SplashDef {
        radius: 10.0,
        damage: 100,
        self_scale: 0.5,
    };

    fn blast(map: &crate::map::Map, c: Vec3, owner: Shooter) -> Blast {
        Blast {
            center: c,
            sector: map.find_sector(c.truncate(), None).unwrap(),
            splash: SPLASH,
            owner,
        }
    }

    fn body(map: &crate::map::Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), 0.4, 1.8).unwrap()
    }

    #[test]
    fn splash_blocked_by_wall() {
        let map = pillar_room();
        let b = blast(&map, Vec3::new(2.0, 5.0, 1.0), Shooter::Actor(5));
        let bodies = [body(&map, 8.0, 5.0), body(&map, 2.0, 8.0)];
        let hits = solve(&map, &b, &bodies, |_| false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].body, 1);
    }

    #[test]
    fn splash_blocked_by_closed_door() {
        let mut map = door_rooms("(kind: Door)", "");
        let c = Vec3::new(2.0, 2.0, 1.0);
        let bodies = [body(&map, 6.0, 2.0)];
        let b = blast(&map, c, Shooter::Actor(5));
        assert_eq!(solve(&map, &b, &bodies, |_| false).len(), 1, "open door");
        Mechanics::new(&mut map);
        assert!(
            solve(&map, &b, &bodies, |_| false).is_empty(),
            "closed door"
        );
    }

    #[test]
    fn splash_reaches_glass_in_view() {
        let map = crate::fixtures::glass_rooms();
        let pane = (0..map.walls.len())
            .filter(|&w| map.walls[w].glass)
            .min()
            .unwrap();
        // In view and in range, from either side of the pane.
        let b = blast(&map, Vec3::new(8.0, 5.0, 1.0), Shooter::Player);
        assert_eq!(walls_in_reach(&map, &b), vec![pane]);
        let b = blast(&map, Vec3::new(13.0, 4.5, 2.5), Shooter::Player);
        assert_eq!(walls_in_reach(&map, &b), vec![pane]);
        // Off to the side of the pane: the nearest point of its opening is its corner.
        let b = blast(&map, Vec3::new(9.0, 9.0, 3.5), Shooter::Player);
        assert_eq!(walls_in_reach(&map, &b), vec![pane]);
        // Out of range.
        let mut b = blast(&map, Vec3::new(8.0, 5.0, 1.0), Shooter::Player);
        b.splash.radius = 1.5;
        assert!(walls_in_reach(&map, &b).is_empty());
        // In range, but the pillar stands between the blast and the pane.
        let b = blast(&map, Vec3::new(2.0, 5.0, 1.0), Shooter::Player);
        assert!(walls_in_reach(&map, &b).is_empty());
        // Panes only: an ordinary room has nothing to shatter.
        let room = pillar_room();
        let b = blast(&room, Vec3::new(1.0, 1.0, 1.0), Shooter::Player);
        assert!(walls_in_reach(&room, &b).is_empty());
    }

    #[test]
    fn splash_reaches_crack_sector() {
        let mut map = door_rooms("(kind: Crack)", "");
        Mechanics::new(&mut map);
        // In range from room A (the crack is x in [4,4.5]) and from room B.
        let b = blast(&map, Vec3::new(2.0, 2.0, 1.0), Shooter::Player);
        assert_eq!(sectors_in_reach(&map, &b), vec![1]);
        let b = blast(&map, Vec3::new(6.0, 1.0, 2.5), Shooter::Player);
        assert_eq!(sectors_in_reach(&map, &b), vec![1]);
        // Out of range.
        let mut b = blast(&map, Vec3::new(2.0, 2.0, 1.0), Shooter::Player);
        b.splash.radius = 1.5;
        assert!(sectors_in_reach(&map, &b).is_empty());
        // Ordinary doors are no cracks.
        let mut doors = door_rooms("(kind: Door)", "");
        Mechanics::new(&mut doors);
        let b = blast(&doors, Vec3::new(2.0, 2.0, 1.0), Shooter::Player);
        assert!(sectors_in_reach(&doors, &b).is_empty());
        // A crack that is already open is not reached again.
        let mut open = door_rooms("(kind: Crack)", "");
        let b = blast(&open, Vec3::new(2.0, 2.0, 1.0), Shooter::Player);
        assert!(sectors_in_reach(&open, &b).is_empty());
        open.sectors[1].ceil_z = 0.0;
        assert_eq!(sectors_in_reach(&open, &b), vec![1]);
    }

    #[test]
    fn crack_behind_a_wall_is_out_of_reach() {
        // Room A (0..4 x 0..4) with a pillar at x in [1,2], y in [1.5,2.5]; crack at x in [4,4.5].
        let mut map = Map::from_ron(
            r#"(name: "t", materials: ["m"],
            vertices: [(0.0,0.0),(4.0,0.0),(4.5,0.0),(4.5,4.0),(4.0,4.0),(0.0,4.0),
                       (1.0,1.5),(1.0,2.5),(2.0,2.5),(2.0,1.5)],
            sectors: [
              (loops: [[0,1,4,5],[6,7,8,9]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
              (loops: [[1,2,3,4]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0,
               mover: Some((kind: Crack))),
            ],
            player_start: (pos: (3.0, 3.0), angle_deg: 0.0))"#,
        )
        .unwrap();
        Mechanics::new(&mut map);
        let behind = blast(&map, Vec3::new(0.5, 2.0, 1.0), Shooter::Player);
        assert!(sectors_in_reach(&map, &behind).is_empty());
        let clear = blast(&map, Vec3::new(3.0, 2.0, 1.0), Shooter::Player);
        assert_eq!(sectors_in_reach(&map, &clear), vec![1]);
    }

    #[test]
    fn splash_falloff_linear() {
        let map = pillar_room();
        let mut b = blast(&map, Vec3::new(1.0, 1.0, 1.0), Shooter::Actor(5));
        b.splash.radius = 6.0;
        b.splash.damage = 100;
        // Surface distances 0 (inside), 1.6 -> 3.0 ... pick x so d = 3 and d = 6.
        let bodies = [
            body(&map, 1.0, 1.0),
            body(&map, 4.4, 1.0), // d = 3.4 - 0.4 = 3.0
            body(&map, 7.4, 1.0), // d = 6.0, at the radius
        ];
        let hits = solve(&map, &b, &bodies, |_| false);
        assert_eq!(
            hits,
            vec![
                BlastHit {
                    body: 0,
                    damage: 100
                },
                BlastHit {
                    body: 1,
                    damage: 50
                },
            ]
        );
        assert!(
            solve(&map, &b, &bodies, |i| i == 1)
                .iter()
                .all(|h| h.body != 1)
        );
    }

    #[test]
    fn rocket_self_damage_scaled() {
        let map = pillar_room();
        let b = blast(&map, Vec3::new(1.0, 1.0, 1.0), Shooter::Player);
        let bodies = [body(&map, 1.0, 1.0), body(&map, 1.0, 1.0)];
        let hits = solve(&map, &b, &bodies, |_| false);
        assert_eq!(
            hits[0],
            BlastHit {
                body: 0,
                damage: 50
            }
        );
        assert_eq!(
            hits[1],
            BlastHit {
                body: 1,
                damage: 100
            }
        );
        let a = blast(&map, Vec3::new(1.0, 1.0, 1.0), Shooter::Actor(0));
        let hits = solve(&map, &a, &bodies, |_| false);
        assert_eq!(hits[1].damage, 50);
        assert_eq!(hits[0].damage, 100);
    }
}
