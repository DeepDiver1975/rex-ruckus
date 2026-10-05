//! Generic projectiles (Grunt laser bolts now, rockets later). Stepped by the combat
//! orchestrator, which owns ids and event emission.

use crate::collide::Body;
use crate::defs::SplashDef;
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
    /// Downward acceleration (m/s^2), applied before each move.
    pub gravity: f32,
    /// Restitution: `Some(e)` bounces off the world instead of ending on a world hit.
    pub bounce: Option<f32>,
    /// Remote-fuse: `life` never runs down, so it never expires.
    pub remote: bool,
    pub splash: Option<SplashDef>,
    /// Settled on a floor; `step_projectile` leaves it where it is.
    pub resting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectileStep {
    Flying,
    HitWorld(Hit),
    /// Index into the bodies slice; use `bodies[i].sector` for its sector.
    HitBody(usize),
    Expired,
    /// A bouncing projectile that has come to rest (reported on every step from then on).
    Resting,
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

/// Most world bounces handled within one step; any motion left after that is dropped.
const MAX_BOUNCES: usize = 3;
/// A projectile bouncing off a floor this flat (normal.z) ...
const FLOOR_NORMAL_Z: f32 = 0.7;
/// ... slower than this (m/s) comes to rest.
const REST_SPEED: f32 = 0.5;
/// Slack on the nudge-height rest test, covering the discrete-step overshoot of the impact.
const NUDGE_SLACK: f32 = 1.5;
/// Gap left between a bouncing projectile's surface and the wall it hit (m).
const BOUNCE_NUDGE: f32 = 0.01;

/// Advances `p` by `dt`: gravity first, then the move. A body or world hit within the step ends
/// it (`pos` is set to the hit point), except that a projectile with `bounce` reflects off world
/// surfaces and keeps flying (at most `MAX_BOUNCES` per step), reporting `Resting` once it
/// settles on a floor. The trace skips every body for which `p.ignores(i)` or the caller's
/// `skip` is true (use `skip` for e.g. dead actors), so callers need not repeat the
/// owner/targets rules. Remote projectiles never expire.
pub fn step_projectile(
    map: &Map,
    p: &mut Projectile,
    bodies: &[Body],
    skip: impl Fn(usize) -> bool,
    dt: f32,
) -> ProjectileStep {
    p.prev = p.pos;
    if p.resting {
        return ProjectileStep::Resting;
    }
    p.vel.z -= p.gravity * dt;
    let mut left = dt;
    for _ in 0..MAX_BOUNCES {
        let speed = p.vel.length();
        if speed <= 0.0 || left <= 0.0 {
            break;
        }
        let ray = Ray {
            origin: p.pos,
            dir: p.vel / speed,
            sector: p.sector,
            max: speed * left,
        };
        let Some(hit) = trace(map, &ray, bodies, |i| p.ignores(i) || skip(i), p.radius) else {
            p.pos += p.vel * left;
            match map.find_sector(p.pos.truncate(), Some(p.sector)) {
                Some(s) => p.sector = s,
                // `pos` has already advanced but `sector` is stale here.
                None => return ProjectileStep::Expired,
            }
            break;
        };
        p.pos = hit.point;
        if let HitKind::Body(i) = hit.kind {
            return ProjectileStep::HitBody(i);
        }
        let Some(e) = p.bounce else {
            return ProjectileStep::HitWorld(hit);
        };
        left -= hit.dist / speed;
        let n = hit.normal;
        p.vel = (p.vel - 2.0 * p.vel.dot(n) * n) * e;
        // The nudge can cross a portal edge (step face, soffit); re-resolve the sector from the
        // nudged point, and if it is outside the map keep the hit point.
        let nudged = hit.point + n * (p.radius + BOUNCE_NUDGE);
        match map.find_sector(nudged.truncate(), Some(hit.sector)) {
            Some(s) => {
                p.pos = nudged;
                p.sector = s;
            }
            None => p.sector = hit.sector,
        }
        // The nudge lifts the centre `radius + BOUNCE_NUDGE` off the floor, and the next fall from
        // that height re-injects energy every bounce. Rest once a bounce can rise no higher than
        // that (with slack for the step size), else the bomb would hop forever.
        let nudge_speed = e * (2.0 * p.gravity * (p.radius + BOUNCE_NUDGE)).sqrt();
        if n.z > FLOOR_NORMAL_Z && p.vel.length() < REST_SPEED.max(NUDGE_SLACK * nudge_speed) {
            p.vel = Vec3::ZERO;
            p.resting = true;
            return ProjectileStep::Resting;
        }
    }
    if !p.remote {
        p.life -= dt;
        if p.life <= 0.0 {
            return ProjectileStep::Expired;
        }
    }
    ProjectileStep::Flying
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
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        }
    }

    fn bomb(map: &Map, pos: Vec3, vel: Vec3) -> Projectile {
        let mut p = bolt(map, pos, vel);
        p.owner = Shooter::Player;
        p.targets = Targets::All;
        p.radius = 0.1;
        p.gravity = 12.0;
        p.bounce = Some(0.45);
        p.remote = true;
        p
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

    #[test]
    fn gravity_arcs_projectile() {
        let map = combat_room();
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 3.0), Vec3::new(5.0, 0.0, 0.0));
        p.gravity = 10.0;
        for _ in 0..30 {
            assert_eq!(
                step_projectile(&map, &mut p, &[], |_| false, DT),
                ProjectileStep::Flying
            );
        }
        // Semi-implicit Euler: vz = -g t exactly, drop slightly over 0.5 g t^2 (~1.25 m).
        assert!((p.vel.z + 5.0).abs() < 1e-3, "{:?}", p.vel);
        assert!((p.vel.x - 5.0).abs() < 1e-6);
        let drop = 3.0 - p.pos.z;
        assert!(drop > 1.2 && drop < 1.35, "{drop}");
        assert!((p.pos.x - 3.5).abs() < 1e-3);
    }

    #[test]
    fn bomb_bounces_and_settles() {
        let map = combat_room();
        let mut p = bomb(&map, Vec3::new(1.0, 1.0, 2.0), Vec3::new(1.0, 0.0, 0.0));
        let mut bounced = false;
        let mut settled = None;
        let mut prev_vz = 0.0;
        for t in 0..1200 {
            match step_projectile(&map, &mut p, &[], |_| false, DT) {
                ProjectileStep::Flying => {
                    if prev_vz < 0.0 && p.vel.z > 0.0 {
                        bounced = true;
                        assert!(p.pos.z > 0.1, "nudged off the floor: {:?}", p.pos);
                    }
                    prev_vz = p.vel.z;
                }
                ProjectileStep::Resting => {
                    settled = Some(t);
                    break;
                }
                o => panic!("a remote bomb never ends: {o:?}"),
            }
        }
        assert!(bounced);
        assert!(settled.is_some(), "never settled");
        assert!(p.resting && p.vel == Vec3::ZERO);
        let at = p.pos;
        for _ in 0..10 {
            assert_eq!(
                step_projectile(&map, &mut p, &[], |_| false, DT),
                ProjectileStep::Resting
            );
        }
        assert_eq!(p.pos, at);
    }

    #[test]
    fn bounce_reflects_off_wall_with_restitution() {
        let map = combat_room();
        // South wall at y=0, normal +y.
        let mut p = bolt(&map, Vec3::new(1.0, 0.2, 1.5), Vec3::new(2.0, -20.0, 0.0));
        p.bounce = Some(0.5);
        let s = step_projectile(&map, &mut p, &[], |_| false, DT);
        assert_eq!(s, ProjectileStep::Flying);
        assert!(
            (p.vel - Vec3::new(1.0, 10.0, 0.0)).length() < 1e-4,
            "{:?}",
            p.vel
        );
        assert!(p.pos.y > 0.0, "{:?}", p.pos);
    }

    #[test]
    fn bounce_off_step_face_keeps_sector_consistent() {
        // Sector 1 (x 4..8) has floor 1.0: its west face at x=4 is a step the bomb hits from
        // sector 0 at z=0.5, and a grazing shot may nudge across the edge.
        let map = crate::fixtures::two_rooms(1.0, 3.0);
        for (vy, z) in [(0.0, 0.5), (30.0, 0.5), (-30.0, 0.9), (5.0, 0.2)] {
            let mut p = bomb(&map, Vec3::new(3.0, 2.0, z), Vec3::new(20.0, vy, 0.0));
            p.gravity = 0.0;
            for _ in 0..120 {
                let o = step_projectile(&map, &mut p, &[], |_| false, DT);
                assert!(
                    matches!(o, ProjectileStep::Flying | ProjectileStep::Resting),
                    "{o:?}"
                );
                assert!(
                    map.sector_contains(p.sector, p.pos.truncate()),
                    "sector {} does not contain {:?}",
                    p.sector,
                    p.pos
                );
            }
        }
    }

    #[test]
    fn remote_projectile_never_expires() {
        let map = combat_room();
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.5), Vec3::new(0.0, 0.0, 0.0));
        p.life = 0.1;
        p.remote = true;
        for _ in 0..60 {
            assert_eq!(
                step_projectile(&map, &mut p, &[], |_| false, DT),
                ProjectileStep::Flying
            );
        }
        assert_eq!(p.life, 0.1);
    }

    #[test]
    fn rocket_hits_actor_body() {
        let map = combat_room();
        // Bodies: [player, actor 0]. A player-owned rocket aimed at actor 0 passes through
        // the player's own body and hits the actor's cylinder.
        let bodies = [body_at(&map, 1.0, 1.0), body_at(&map, 5.0, 1.0)];
        let mut p = bolt(&map, Vec3::new(1.0, 1.0, 1.0), Vec3::new(30.0, 0.0, 0.0));
        p.owner = Shooter::Player;
        p.targets = Targets::All;
        let mut out = ProjectileStep::Flying;
        for _ in 0..30 {
            out = step_projectile(&map, &mut p, &bodies, |_| false, DT);
            if out != ProjectileStep::Flying {
                break;
            }
        }
        assert_eq!(out, ProjectileStep::HitBody(1));
        assert!((p.pos.x - (5.0 - 0.35 - 0.05)).abs() < 1e-2, "{:?}", p.pos);
    }
}
