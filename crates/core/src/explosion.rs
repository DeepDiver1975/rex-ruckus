//! Splash damage: which bodies an explosion reaches and for how much.

use crate::collide::Body;
use crate::defs::SplashDef;
use crate::map::SectorId;
use crate::projectile::Shooter;
use crate::trace::can_see;
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
    map: &crate::map::Map,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{door_rooms, pillar_room};
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
