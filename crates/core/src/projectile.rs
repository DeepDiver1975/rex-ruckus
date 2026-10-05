//! Generic projectiles (Grunt laser bolts now, rockets later). Stepped by the combat
//! orchestrator, which owns ids and event emission.

use crate::collide::Body;
use crate::map::{Map, SectorId};
use crate::trace::{Hit, HitKind, Ray, trace};
use glam::Vec3;

/// Index of the player in the combat `bodies` slice `[player, actor 0, actor 1, ..]`.
const PLAYER_BODY: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shooter {
    Player,
    Actor(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Targets {
    Player,
    Actors,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Projectile {
    /// Assigned by the caller; never changed by `step_projectile`.
    pub id: u32,
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub sector: SectorId,
    pub radius: f32,
    pub damage: i32,
    pub owner: Shooter,
    pub targets: Targets,
    /// Seconds remaining.
    pub life: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectileStep {
    Flying,
    HitWorld(Hit),
    /// Index into the bodies slice; use `bodies[i].sector` for its sector.
    HitBody(usize),
    Expired,
}

impl Projectile {
    /// Whether body `i` (combat convention `[player, actor 0, ..]`) can never be hit by this
    /// projectile: it is the owner, or not among `targets`.
    pub fn ignores(&self, i: usize) -> bool {
        let owner = match self.owner {
            Shooter::Player => i == PLAYER_BODY,
            Shooter::Actor(a) => i == a + 1,
        };
        let non_target = match self.targets {
            Targets::Player => i != PLAYER_BODY,
            Targets::Actors => i == PLAYER_BODY,
            Targets::All => false,
        };
        owner || non_target
    }
}

/// Advances `p` by `dt`. A body or world hit within the step ends it (`pos` is set to the hit
/// point). The trace skips every body for which `p.ignores(i)` or the caller's `skip` is true
/// (use `skip` for e.g. dead actors), so callers need not repeat the owner/targets rules.
pub fn step_projectile(
    map: &Map,
    p: &mut Projectile,
    bodies: &[Body],
    skip: impl Fn(usize) -> bool,
    dt: f32,
) -> ProjectileStep {
    p.prev = p.pos;
    let speed = p.vel.length();
    if speed > 0.0 && dt > 0.0 {
        let ray = Ray {
            origin: p.pos,
            dir: p.vel / speed,
            sector: p.sector,
            max: speed * dt,
        };
        if let Some(hit) = trace(map, &ray, bodies, |i| p.ignores(i) || skip(i), p.radius) {
            p.pos = hit.point;
            return match hit.kind {
                HitKind::Body(i) => ProjectileStep::HitBody(i),
                _ => ProjectileStep::HitWorld(hit),
            };
        }
        p.pos += p.vel * dt;
        match map.find_sector(p.pos.truncate(), Some(p.sector)) {
            Some(s) => p.sector = s,
            // `pos` has already advanced but `sector` is stale here.
            None => return ProjectileStep::Expired,
        }
    }
    p.life -= dt;
    if p.life <= 0.0 {
        ProjectileStep::Expired
    } else {
        ProjectileStep::Flying
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::combat_room;
    use crate::mechanics::Mechanics;
    use glam::Vec2;

    const DT: f32 = 1.0 / 60.0;

    fn bolt(map: &Map, pos: Vec3, vel: Vec3) -> Projectile {
        Projectile {
            id: 7,
            pos,
            prev: pos,
            vel,
            sector: map.find_sector(pos.truncate(), None).unwrap(),
            radius: 0.05,
            damage: 10,
            owner: Shooter::Actor(0),
            targets: Targets::Player,
            life: 5.0,
        }
    }

    fn body_at(map: &Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), 0.35, 1.8).unwrap()
    }

    #[test]
    fn bolt_flies_straight_at_speed() {
        let map = combat_room();
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.5), Vec3::new(10.0, 0.0, 0.0));
        for _ in 0..30 {
            assert_eq!(
                step_projectile(&map, &mut p, &[], |_| false, DT),
                ProjectileStep::Flying
            );
        }
        assert!((p.pos - Vec3::new(6.0, 1.0, 1.5)).length() < 1e-3);
        assert!(((p.pos - p.prev).length() - 10.0 * DT).abs() < 1e-4);
        assert_eq!(p.id, 7);
    }

    #[test]
    fn bolt_hits_wall_and_reports_point() {
        let map = combat_room();
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.5), Vec3::new(0.0, -30.0, 0.0));
        let mut out = ProjectileStep::Flying;
        for _ in 0..30 {
            out = step_projectile(&map, &mut p, &[], |_| false, DT);
            if out != ProjectileStep::Flying {
                break;
            }
        }
        // The south wall of the main room sits at y=0.
        match out {
            ProjectileStep::HitWorld(h) => {
                assert!((h.point - Vec3::new(1.0, 0.0, 1.5)).length() < 1e-3);
                assert_eq!(p.pos, h.point);
            }
            o => panic!("expected world hit, got {o:?}"),
        }
    }

    #[test]
    fn bolt_crosses_open_portal_and_updates_sector() {
        let map = combat_room();
        // North through the portal at y=5 (x 8..12) into sector 2 (ceiling 2.5).
        let mut p = bolt(&map, Vec3::new(10.0, 4.9, 1.5), Vec3::new(0.0, 6.0, 0.0));
        assert_eq!(p.sector, 0);
        let s = step_projectile(&map, &mut p, &[], |_| false, 0.1);
        assert_eq!(s, ProjectileStep::Flying);
        assert!(p.pos.y > 5.0);
        assert_eq!(p.sector, map.find_sector(p.pos.truncate(), None).unwrap());
        assert_eq!(p.sector, 2);
    }

    #[test]
    fn bolt_stops_at_closed_door() {
        let mut map = combat_room();
        Mechanics::new(&mut map);
        let mut p = bolt(&map, Vec3::new(11.0, 4.0, 1.5), Vec3::new(40.0, 0.0, 0.0));
        let mut out = ProjectileStep::Flying;
        for _ in 0..60 {
            out = step_projectile(&map, &mut p, &[], |_| false, DT);
            if out != ProjectileStep::Flying {
                break;
            }
        }
        match out {
            ProjectileStep::HitWorld(h) => assert!(h.point.x < 12.6 && h.point.x > 11.9),
            o => panic!("expected world hit, got {o:?}"),
        }
        assert!(p.pos.x < 12.6);
    }

    #[test]
    fn bolt_ignores_owner_and_non_targets() {
        let map = combat_room();
        // Bodies: [player, actor0, actor1]. Eye-height bolt passes through chest-high bodies.
        let player = body_at(&map, 6.0, 1.0);
        let a0 = body_at(&map, 3.0, 1.0);
        let a1 = body_at(&map, 4.5, 1.0);
        let bodies = [player, a0, a1];
        let start = Vec3::new(1.0, 1.0, 1.0);
        let vel = Vec3::new(40.0, 0.0, 0.0);
        let run = |p: &mut Projectile| {
            for _ in 0..60 {
                match step_projectile(&map, p, &bodies, |_| false, DT) {
                    ProjectileStep::Flying => {}
                    o => return o,
                }
            }
            ProjectileStep::Flying
        };
        // Actor 0 shoots at the player: starts inside its own body, skips actor 1 too.
        let mut p = bolt(&map, Vec3::new(3.0, 1.0, 1.0), vel);
        assert_eq!(run(&mut p), ProjectileStep::HitBody(0));
        // Actors-targeting bolt from the player never hits body 0 (player) and hits actor 0.
        let mut p = bolt(&map, start, vel);
        p.owner = Shooter::Player;
        p.targets = Targets::Actors;
        assert_eq!(run(&mut p), ProjectileStep::HitBody(1));
        // Owner actor 0 with All targets skips body 1 (itself) and hits actor 1 (body 2).
        let mut p = bolt(&map, Vec3::new(3.0, 1.0, 1.0), vel);
        p.targets = Targets::All;
        assert_eq!(run(&mut p), ProjectileStep::HitBody(2));
        // Caller skip is honoured as well.
        let mut p = bolt(&map, Vec3::new(3.0, 1.0, 1.0), vel);
        assert!(p.ignores(1) && p.ignores(2) && !p.ignores(0));
        p.targets = Targets::All;
        let o = step_projectile(&map, &mut p, &bodies, |i| i == 0 || i == 2, 0.5);
        match o {
            ProjectileStep::HitWorld(h) => assert!((h.point.x - 12.0).abs() < 1e-3),
            o => panic!("expected world hit at x=12, got {o:?}"),
        }
    }

    #[test]
    fn fast_bolt_does_not_tunnel_through_thin_body() {
        let map = combat_room();
        let mut thin = body_at(&map, 4.0, 1.0);
        thin.radius = 0.05;
        let bodies = [thin];
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.0), Vec3::new(200.0, 0.0, 0.0));
        p.targets = Targets::All;
        p.owner = Shooter::Actor(5);
        p.radius = 0.0;
        // One step covers 3.33 m; the body is 0.1 m wide and mid-step.
        let o = step_projectile(&map, &mut p, &bodies, |_| false, DT);
        assert_eq!(o, ProjectileStep::HitBody(0));
        assert!((p.pos.x - 3.95).abs() < 1e-3);
    }

    #[test]
    fn bolt_expires_after_life() {
        let map = combat_room();
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.5), Vec3::new(1.0, 0.0, 0.0));
        p.life = 0.25;
        let mut steps = 0;
        loop {
            steps += 1;
            match step_projectile(&map, &mut p, &[], |_| false, 0.1) {
                ProjectileStep::Flying => {}
                ProjectileStep::Expired => break,
                o => panic!("unexpected {o:?}"),
            }
            assert!(steps < 10);
        }
        assert_eq!(steps, 3);
    }

    #[test]
    fn ignores_truth_table() {
        use Shooter::{Actor, Player};
        // (owner, targets, ignored flags for bodies 0, 1, 2)
        let cases = [
            (Player, Targets::Player, [true, true, true]),
            (Player, Targets::Actors, [true, false, false]),
            (Player, Targets::All, [true, false, false]),
            (Actor(0), Targets::Player, [false, true, true]),
            (Actor(0), Targets::Actors, [true, true, false]),
            (Actor(0), Targets::All, [false, true, false]),
        ];
        let map = combat_room();
        for (owner, targets, want) in cases {
            let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.0), Vec3::X);
            p.owner = owner;
            p.targets = targets;
            let got = [p.ignores(0), p.ignores(1), p.ignores(2)];
            assert_eq!(got, want, "{owner:?} / {targets:?}");
        }
    }

    #[test]
    fn actors_targeting_bolt_passes_player_in_front() {
        let map = combat_room();
        let bodies = [body_at(&map, 3.0, 1.0), body_at(&map, 5.0, 1.0)];
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.0), Vec3::new(40.0, 0.0, 0.0));
        p.owner = Shooter::Actor(7);
        p.targets = Targets::Actors;
        let mut out = ProjectileStep::Flying;
        for _ in 0..30 {
            out = step_projectile(&map, &mut p, &bodies, |_| false, DT);
            if out != ProjectileStep::Flying {
                break;
            }
        }
        assert_eq!(out, ProjectileStep::HitBody(1));
    }

    #[test]
    fn hit_wins_over_expiry_in_same_step() {
        let map = combat_room();
        // World hit: wall at y=0, 1 m away, step covers 3 m, life runs out this step.
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.5), Vec3::new(0.0, -30.0, 0.0));
        p.life = 0.05;
        let o = step_projectile(&map, &mut p, &[], |_| false, 0.1);
        assert!(matches!(o, ProjectileStep::HitWorld(_)), "{o:?}");
        // Body hit.
        let bodies = [body_at(&map, 2.0, 1.0)];
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.0), Vec3::new(30.0, 0.0, 0.0));
        p.life = 0.05;
        let o = step_projectile(&map, &mut p, &bodies, |_| false, 0.1);
        assert_eq!(o, ProjectileStep::HitBody(0));
    }
}
