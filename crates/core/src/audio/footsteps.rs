//! Footstep and landing detection from the player body, fed once per fixed tick.

use super::Cue;
use crate::collide::Body;
use glam::Vec3;

/// A horizontal jump larger than this in one tick is a teleport (e.g. a respawn), not walking.
const TELEPORT: f32 = 3.0;

/// Turns grounded horizontal travel into `Footstep` cues and hard touchdowns into `Land`.
#[derive(Debug, Clone, Default)]
pub struct Footsteps {
    /// Grounded horizontal distance since the last step, metres.
    stride: f32,
    was_grounded: bool,
    last_pos: Option<Vec3>,
    /// Fastest downward speed (positive, m/s) seen since leaving the ground. The body's
    /// vertical velocity is usually already zeroed on the touchdown tick, so the peak is kept.
    peak_fall: f32,
}

impl Footsteps {
    /// Metres of grounded horizontal travel per step.
    pub const STRIDE: f32 = 1.6;
    /// Downward speed (m/s) at touchdown needed for a `Land` cue.
    pub const LAND_SPEED: f32 = 4.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds the player body for one tick.
    pub fn tick_body(&mut self, body: &Body) -> Option<Cue> {
        self.tick(body.pos, body.vel.z, body.on_ground)
    }

    /// Feeds one tick. Returns at most one cue. The first tick only records state.
    pub fn tick(&mut self, pos: Vec3, vel_z: f32, grounded: bool) -> Option<Cue> {
        let prev = self.last_pos.replace(pos);
        let was_grounded = std::mem::replace(&mut self.was_grounded, grounded);
        let Some(prev) = prev else {
            self.peak_fall = 0.0;
            return None;
        };

        if (pos - prev).truncate().length() > TELEPORT {
            self.stride = 0.0;
            self.peak_fall = 0.0;
            return None;
        }

        if !grounded {
            self.peak_fall = self.peak_fall.max(-vel_z);
            return None;
        }

        if !was_grounded {
            let fall = self.peak_fall.max(-vel_z);
            self.peak_fall = 0.0;
            self.stride = 0.0;
            return (fall >= Self::LAND_SPEED).then_some(Cue::Land);
        }

        self.stride += (pos - prev).truncate().length();
        if self.stride >= Self::STRIDE {
            self.stride -= Self::STRIDE;
            return Some(Cue::Footstep);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(f: &mut Footsteps, metres: f32, step: f32) -> Vec<Cue> {
        let mut x = f.last_pos.map_or(0.0, |p| p.x);
        let n = (metres / step).round() as usize;
        (0..n)
            .filter_map(|_| {
                x += step;
                f.tick(Vec3::new(x, 0.0, 0.0), 0.0, true)
            })
            .collect()
    }

    fn primed() -> Footsteps {
        let mut f = Footsteps::new();
        assert_eq!(f.tick(Vec3::ZERO, 0.0, true), None);
        f
    }

    #[test]
    fn one_stride_is_one_step_and_remainder_carries() {
        let mut f = primed();
        assert_eq!(walk(&mut f, 1.6, 0.1), vec![Cue::Footstep]);
        let mut f = primed();
        assert_eq!(walk(&mut f, 4.0, 0.1).len(), 2);
    }

    #[test]
    fn standing_or_airborne_travel_is_silent() {
        let mut f = primed();
        for _ in 0..100 {
            assert_eq!(f.tick(Vec3::ZERO, 0.0, true), None);
        }
        let mut f = primed();
        let mut x = 0.0;
        for _ in 0..100 {
            x += 0.1;
            assert_eq!(f.tick(Vec3::new(x, 0.0, 1.0), 0.0, false), None);
        }
    }

    #[test]
    fn hard_landing_cues_and_resets_stride() {
        let mut f = primed();
        assert_eq!(walk(&mut f, 1.0, 0.1).len(), 0);
        f.tick(Vec3::new(1.0, 0.0, 2.0), -2.0, false);
        f.tick(Vec3::new(1.0, 0.0, 1.0), -6.0, false);
        // Touchdown tick: velocity already zeroed, the peak is remembered.
        assert_eq!(f.tick(Vec3::new(1.0, 0.0, 0.0), 0.0, true), Some(Cue::Land));
        // Stride restarted: 1.0 m more would have stepped had it carried.
        assert!(walk(&mut f, 1.0, 0.1).is_empty());
        assert_eq!(walk(&mut f, 0.7, 0.1), vec![Cue::Footstep]);
    }

    #[test]
    fn soft_landing_is_silent_but_resets() {
        let mut f = primed();
        walk(&mut f, 1.0, 0.1);
        f.tick(Vec3::new(1.0, 0.0, 0.5), -2.0, false);
        assert_eq!(f.tick(Vec3::new(1.0, 0.0, 0.0), 0.0, true), None);
        assert!(walk(&mut f, 1.0, 0.1).is_empty());
    }

    #[test]
    fn teleport_is_silent_and_resets() {
        let mut f = primed();
        walk(&mut f, 1.0, 0.1);
        assert_eq!(f.tick(Vec3::new(50.0, 0.0, 0.0), 0.0, true), None);
        assert!(walk(&mut f, 1.0, 0.1).is_empty());
        assert_eq!(walk(&mut f, 0.7, 0.1), vec![Cue::Footstep]);
    }

    #[test]
    fn first_tick_never_emits() {
        for grounded in [true, false] {
            let mut f = Footsteps::new();
            assert_eq!(f.tick(Vec3::new(9.0, 9.0, 0.0), -20.0, grounded), None);
        }
    }

    #[test]
    fn tick_body_uses_body_fields() {
        let mut b = Body {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            radius: 0.3,
            height: 1.8,
            sector: Default::default(),
            on_ground: true,
        };
        let mut f = Footsteps::new();
        assert_eq!(f.tick_body(&b), None);
        b.pos.x = 1.7;
        assert_eq!(f.tick_body(&b), Some(Cue::Footstep));
    }
}
