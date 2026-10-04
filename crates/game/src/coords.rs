//! Core uses right-handed Z-up (x east, y north); Bevy uses right-handed Y-up with −Z forward.
//! The mapping (x, y, z) → (x, z, −y) is a proper rotation, so triangle winding is preserved.

use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

pub fn to_bevy(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y)
}

pub fn to_bevy_arr(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

/// Core heading (0 = east, counter-clockwise) → Bevy yaw about +Y (0 = looking down −Z).
pub fn core_angle_to_yaw(angle: f32) -> f32 {
    angle - FRAC_PI_2
}

/// Unit direction in core XY for a core heading.
pub fn forward_2d(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_maps_to_bevy_forward_and_up_to_y() {
        assert_eq!(to_bevy(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(to_bevy(Vec3::Z), Vec3::Y);
        assert_eq!(to_bevy(Vec3::X), Vec3::X);
    }

    #[test]
    fn camera_yaw_matches_core_heading() {
        for deg in [0.0f32, 45.0, 90.0, 180.0, 270.0] {
            let a = deg.to_radians();
            let bevy_forward = Quat::from_rotation_y(core_angle_to_yaw(a)) * Vec3::NEG_Z;
            let expected = to_bevy(forward_2d(a).extend(0.0));
            assert!(
                bevy_forward.distance(expected) < 1e-5,
                "{deg}°: {bevy_forward} vs {expected}"
            );
        }
    }
}
