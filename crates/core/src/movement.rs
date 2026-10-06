//! Player movement with a Build-era feel: fast, snappy acceleration, strong friction,
//! automatic step-up, floaty jumps.

use crate::collide::{Body, clip_move, z_range};
use crate::map::Map;
use glam::{Vec2, Vec3};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveInput {
    /// Desired direction in core XY; length ≤ 1 (longer is clamped).
    pub wish: Vec2,
    pub jump: bool,
    pub crouch: bool,
    /// Vertical thrust in -1..1 (up positive); only read while `jetpack` is set.
    pub thrust: f32,
    /// Jetpack on: no gravity, no jumping, crouch does not crouch, `thrust` steers the climb.
    pub jetpack: bool,
}

/// Jetpack climb/descent speed at full thrust (m/s).
pub const JETPACK_SPEED: f32 = 5.0;
/// How fast vertical velocity eases toward the thrust target (m/s²).
pub const JETPACK_ACCEL: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub max_speed: f32,
    /// Acceleration as a multiple of `max_speed` per second.
    pub ground_accel: f32,
    pub air_accel: f32,
    pub friction: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    pub step_height: f32,
    pub stand_height: f32,
    pub crouch_height: f32,
    pub crouch_speed_factor: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            max_speed: 9.0,
            ground_accel: 12.0,
            air_accel: 2.0,
            friction: 10.0,
            jump_speed: 6.0,
            gravity: 20.0,
            step_height: 0.55,
            stand_height: 1.8,
            crouch_height: 1.1,
            crouch_speed_factor: 0.5,
        }
    }
}

/// A sector's floor and ceiling heights, `(floor_z, ceil_z)`.
pub(crate) type Pose = (f32, f32);

/// Can a (crouching) walker with tuning `t` go from a sector in pose `a` into one in pose `b`?
/// Shared by level validation (authored reachability) and actor pathing (live heights). A closed
/// door (`ceil_z == floor_z`) never fits.
pub(crate) fn can_cross((fa, ca): Pose, (fb, cb): Pose, t: &Tuning) -> bool {
    fb <= fa + t.step_height
        && cb - fb >= t.crouch_height
        && ca.min(cb) - fa.max(fb) >= t.crouch_height
}

/// How an actor gets from sector to sector: walkers by `can_cross` (step height, crouch), flyers
/// by the opening alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pass {
    Walk(Tuning),
    /// A flyer of this body height: steps in the floor do not matter, only that the opening
    /// (lowest ceiling minus highest floor of the two sectors) is at least `height`.
    Fly {
        height: f32,
    },
}

impl Pass {
    /// Can this mover go from a sector in pose `a` into one in pose `b`?
    pub(crate) fn allows(&self, a: Pose, b: Pose) -> bool {
        match self {
            Pass::Walk(t) => can_cross(a, b, t),
            Pass::Fly { height } => a.1.min(b.1) - a.0.max(b.0) >= *height,
        }
    }
}

/// Lowest a flyer cruises above the floor under it (m).
pub const FLYER_MIN_CLEARANCE: f32 = 0.3;

/// One tick of flying movement: no gravity. Horizontal motion is `wish.truncate()` (length
/// clamped to 1) times `speed`, slid along walls and glass like a walker, but a flyer ignores
/// floor steps: it may enter any sector whose opening fits its height. Vertical: `z` eases toward
/// `floor_z + hover` at up to `speed` m/s and is clamped to
/// `[floor_z + FLYER_MIN_CLEARANCE, ceil_z - height]` of the sector range under the body; where
/// that range is empty the body sits at the (lowered) floor clearance, clipped by the ceiling.
/// `wish.z` is ignored: the height is steered by `hover` alone. `on_ground` is always false.
pub fn step_flyer(map: &Map, body: &mut Body, wish: Vec3, speed: f32, hover: f32, dt: f32) {
    body.on_ground = false;
    let fits = |b: &Body| {
        let (floor, ceil) = z_range(map, b.pos.truncate(), b.radius, b.sector);
        ceil - floor >= b.height
    };
    let was_fitting = fits(body);
    let (start, start_sector, z) = (body.pos, body.sector, body.pos.z);

    // Clip with the feet on the floor and an unlimited step: `clip_move` then only blocks what
    // is too low for the body anywhere, and the check below applies the same opening rule as
    // `Pass::Fly` (lowest ceiling minus highest floor).
    let (floor, _) = z_range(map, body.pos.truncate(), body.radius, body.sector);
    body.pos.z = floor;
    let delta = wish.truncate().clamp_length_max(1.0) * speed * dt;
    clip_move(map, body, delta, f32::MAX);
    body.pos.z = z;
    if was_fitting && !fits(body) {
        // The opening it slid into is too low for its height.
        body.pos.x = start.x;
        body.pos.y = start.y;
        body.sector = start_sector;
    }

    let (floor, ceil) = z_range(map, body.pos.truncate(), body.radius, body.sector);
    let (lo, hi) = (floor + FLYER_MIN_CLEARANCE, ceil - body.height);
    let clamp = |z: f32| {
        if hi >= lo {
            z.clamp(lo, hi)
        } else {
            lo.min(hi).max(floor)
        }
    };
    let eased = z + (floor + hover - z).clamp(-speed * dt, speed * dt);
    body.pos.z = clamp(eased);
    body.vel = ((body.pos - start) / dt).with_z((body.pos.z - z) / dt);
    crate::props::push_out_of_props(map, body, Tuning::default().step_height);
}

pub fn step_player(map: &Map, body: &mut Body, input: &MoveInput, t: &Tuning, dt: f32) {
    // Crouch / stand up (only if there is headroom).
    let (_, ceil_here) = z_range(map, body.pos.truncate(), body.radius, body.sector);
    // With the jetpack on, crouch means descend, not duck.
    if input.crouch && !input.jetpack {
        body.height = t.crouch_height;
    } else if ceil_here - body.pos.z >= t.stand_height {
        body.height = t.stand_height;
    }
    let max_speed = if body.height < t.stand_height {
        t.max_speed * t.crouch_speed_factor
    } else {
        t.max_speed
    };

    // Horizontal: friction first, then accelerate along the wish direction (Quake/Build style).
    let mut v = body.vel.truncate();
    if body.on_ground {
        let speed = v.length();
        if speed > 0.0 {
            v *= (speed - speed * t.friction * dt).max(0.0) / speed;
        }
    }
    let wish = input.wish.clamp_length_max(1.0);
    if wish != Vec2::ZERO {
        let dir = wish.normalize();
        let target = wish.length() * max_speed;
        let accel = if body.on_ground {
            t.ground_accel
        } else {
            t.air_accel
        };
        let add = (target - v.dot(dir)).max(0.0).min(accel * max_speed * dt);
        v += dir * add;
    }

    let before = body.pos.truncate();
    let result = clip_move(map, body, v * dt, t.step_height);
    if result.blocked {
        // Keep the direction actually travelled; a collision (or push-out) never adds speed.
        v = ((body.pos.truncate() - before) / dt).clamp_length_max(v.length());
    }
    body.vel.x = v.x;
    body.vel.y = v.y;

    // Vertical.
    let (floor, ceil) = z_range(map, body.pos.truncate(), body.radius, body.sector);
    if input.jetpack {
        // No gravity and no jumping: ease toward the thrust target. At the floor it hovers.
        let target = input.thrust.clamp(-1.0, 1.0) * JETPACK_SPEED;
        let dv = (target - body.vel.z).clamp(-JETPACK_ACCEL * dt, JETPACK_ACCEL * dt);
        body.vel.z += dv;
        body.pos.z += body.vel.z * dt;
        if body.pos.z <= floor {
            body.pos.z = floor;
            body.vel.z = body.vel.z.max(0.0);
            body.on_ground = true;
        } else {
            body.on_ground = false;
        }
    } else if body.on_ground {
        if input.jump {
            body.vel.z = t.jump_speed;
            body.on_ground = false;
        } else if floor >= body.pos.z - t.step_height {
            // Stick to the ground: covers stepping up and walking down stairs.
            body.pos.z = floor;
            body.vel.z = 0.0;
        } else {
            body.on_ground = false;
        }
    }
    if !input.jetpack && !body.on_ground {
        body.vel.z -= t.gravity * dt;
        body.pos.z += body.vel.z * dt;
        if body.pos.z <= floor {
            body.pos.z = floor;
            body.vel.z = 0.0;
            body.on_ground = true;
        }
    }
    if body.pos.z + body.height > ceil {
        body.pos.z = (ceil - body.height).max(floor);
        body.vel.z = body.vel.z.min(0.0);
    }
    crate::props::push_out_of_props(map, body, t.step_height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::Body;
    use crate::fixtures::{pillar_room, two_rooms};

    const DT: f32 = 1.0 / 60.0;

    fn run(map: &Map, b: &mut Body, input: MoveInput, ticks: usize) {
        let t = Tuning::default();
        for _ in 0..ticks {
            step_player(map, b, &input, &t, DT);
        }
    }

    fn walk(wish: Vec2) -> MoveInput {
        MoveInput {
            wish,
            ..Default::default()
        }
    }

    fn spawn(map: &Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), 0.35, Tuning::default().stand_height).unwrap()
    }

    #[test]
    fn reaches_top_speed_quickly_then_stops() {
        let map = pillar_room();
        let mut b = spawn(&map, 2.0, 1.0);
        run(&map, &mut b, walk(Vec2::Y), 20);
        assert!(
            (b.vel.truncate().length() - Tuning::default().max_speed).abs() < 0.05,
            "{}",
            b.vel
        );
        run(&map, &mut b, MoveInput::default(), 30);
        assert!(b.vel.truncate().length() < 0.1);
    }

    #[test]
    fn jump_leaves_ground_and_lands() {
        let map = pillar_room();
        let mut b = spawn(&map, 2.0, 2.0);
        run(
            &map,
            &mut b,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            1,
        );
        assert!(!b.on_ground && b.vel.z > 0.0);
        run(&map, &mut b, MoveInput::default(), 90);
        assert!(b.on_ground);
        assert_eq!(b.pos.z, 0.0);
    }

    #[test]
    fn walks_up_a_low_step() {
        let map = two_rooms(0.4, 3.0);
        let mut b = spawn(&map, 2.0, 2.0);
        run(&map, &mut b, walk(Vec2::X), 60);
        assert_eq!(b.sector, 1);
        assert_eq!(b.pos.z, 0.4);
        assert!(b.on_ground);
    }

    #[test]
    fn high_ledge_blocks() {
        let map = two_rooms(1.0, 3.0);
        let mut b = spawn(&map, 2.0, 2.0);
        run(&map, &mut b, walk(Vec2::X), 60);
        assert_eq!(b.sector, 0);
    }

    #[test]
    fn walking_off_a_ledge_falls() {
        let map = two_rooms(1.0, 3.0);
        let mut b = spawn(&map, 6.0, 2.0);
        let mut fell = false;
        let t = Tuning::default();
        for _ in 0..60 {
            step_player(&map, &mut b, &walk(Vec2::NEG_X), &t, DT);
            fell |= !b.on_ground;
        }
        assert!(fell);
        assert_eq!((b.sector, b.pos.z, b.on_ground), (0, 0.0, true));
    }

    #[test]
    fn head_bumps_low_ceiling() {
        let map = two_rooms(0.0, 2.0);
        let mut b = spawn(&map, 6.0, 2.0);
        let t = Tuning::default();
        for i in 0..60 {
            step_player(
                &map,
                &mut b,
                &MoveInput {
                    jump: i == 0,
                    ..Default::default()
                },
                &t,
                DT,
            );
            assert!(
                b.pos.z + b.height <= 2.0 + 1e-4,
                "tick {i}: head at {}",
                b.pos.z + b.height
            );
        }
    }

    #[test]
    fn crouching_fits_under_low_ceiling_and_stays_crouched() {
        let map = two_rooms(0.0, 1.5);
        let mut b = spawn(&map, 2.0, 2.0);
        run(&map, &mut b, walk(Vec2::X), 60);
        assert_eq!(b.sector, 0, "standing must not fit");
        run(
            &map,
            &mut b,
            MoveInput {
                wish: Vec2::X,
                crouch: true,
                ..Default::default()
            },
            90,
        );
        assert_eq!(b.sector, 1, "crouching must fit");
        run(&map, &mut b, MoveInput::default(), 5);
        assert_eq!(
            b.height,
            Tuning::default().crouch_height,
            "no room to stand up"
        );
    }

    fn jet(thrust: f32) -> MoveInput {
        MoveInput {
            thrust,
            jetpack: true,
            ..Default::default()
        }
    }

    #[test]
    fn jetpack_hover_holds_height() {
        let map = pillar_room();
        let mut b = spawn(&map, 2.0, 2.0);
        run(&map, &mut b, jet(1.0), 30);
        assert!(b.pos.z > 0.5 && !b.on_ground, "climbing: {}", b.pos);
        run(&map, &mut b, jet(0.0), 60);
        let z = b.pos.z;
        run(&map, &mut b, jet(0.0), 120);
        assert_eq!(b.pos.z, z, "hover must not sink");
        assert_eq!(b.vel.z, 0.0);
        // Crouch means descend, not duck.
        run(
            &map,
            &mut b,
            MoveInput {
                crouch: true,
                ..jet(-1.0)
            },
            10,
        );
        assert!(b.pos.z < z && b.height == Tuning::default().stand_height);
        // Jump is ignored on the ground.
        run(&map, &mut b, jet(-1.0), 200);
        assert_eq!((b.pos.z, b.on_ground), (0.0, true));
        run(
            &map,
            &mut b,
            MoveInput {
                jump: true,
                ..jet(0.0)
            },
            10,
        );
        assert_eq!(b.pos.z, 0.0, "hovers at the floor");
    }

    #[test]
    fn jetpack_climbs_and_stops_at_ceiling() {
        let map = two_rooms(0.0, 3.0);
        let mut b = spawn(&map, 2.0, 2.0);
        for i in 0..240 {
            run(&map, &mut b, jet(1.0), 1);
            assert!(b.pos.z + b.height <= 3.0 + 1e-4, "tick {i}: {}", b.pos);
        }
        assert!((b.pos.z + b.height - 3.0).abs() < 1e-3, "{}", b.pos);
        assert!(b.vel.z <= 0.0);
    }

    #[test]
    fn gravity_resumes_when_fuel_out() {
        let map = pillar_room();
        let mut b = spawn(&map, 2.0, 2.0);
        run(&map, &mut b, jet(1.0), 40);
        run(&map, &mut b, jet(0.0), 60);
        let z = b.pos.z;
        run(&map, &mut b, MoveInput::default(), 1);
        assert!(b.vel.z < 0.0 && b.pos.z < z, "falls on the very step");
        run(&map, &mut b, MoveInput::default(), 120);
        assert_eq!((b.pos.z, b.on_ground), (0.0, true));
    }

    #[test]
    fn push_out_does_not_add_speed() {
        let map = two_rooms(0.0, 3.0);
        let mut b = spawn(&map, 2.0, 2.0);
        b.pos.x = 0.1;
        step_player(&map, &mut b, &MoveInput::default(), &Tuning::default(), DT);
        assert!(b.pos.x > 0.34, "pushed out: {}", b.pos);
        assert!(b.vel.truncate().length() < 1e-3, "no kick: {}", b.vel);
    }
}
