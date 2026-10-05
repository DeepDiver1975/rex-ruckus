mod common;

use proptest::prelude::*;
use rr_core::collide::Body;
use rr_core::fixtures::combat_room;
use rr_core::glam::Vec3;
use rr_core::mechanics::Mechanics;
use rr_core::projectile::{Projectile, ProjectileStep, Shooter, Targets, step_projectile};

const DT: f32 = 1.0 / 60.0;
const RADIUS: f32 = 0.1;
const TOL: f32 = 1e-3;

/// Start points are rejection-sampled (pillar hole, step and soffit heights), so the reject
/// budget scales with the case count instead of proptest's fixed default of 1024.
fn config() -> ProptestConfig {
    let cases = common::cases(128);
    ProptestConfig {
        max_global_rejects: cases.saturating_mul(4),
        ..ProptestConfig::with_cases(cases)
    }
}

proptest! {
    #![proptest_config(config())]

    /// A bouncing, gravity-driven bomb never leaves the map: every tick its position lies in
    /// some sector and between that sector's floor and ceiling (plus the bomb's radius).
    #[test]
    fn bomb_never_tunnels(
        doors_closed in any::<bool>(),
        x in 0.5f32..15.5,
        y in 0.5f32..7.5,
        z in 0.6f32..3.4,
        vx in -40.0f32..40.0,
        vy in -40.0f32..40.0,
        vz in -40.0f32..40.0,
    ) {
        let mut map = combat_room();
        if doors_closed {
            Mechanics::new(&mut map);
        }
        let pos = Vec3::new(x, y, z);
        let start = map.find_sector(pos.truncate(), None);
        prop_assume!(start.is_some());
        let sector = start.unwrap();
        let sec = &map.sectors[sector];
        prop_assume!(sec.floor_z + RADIUS < z && z < sec.ceil_z - RADIUS);
        let mut p = Projectile {
            id: 1,
            pos,
            prev: pos,
            vel: Vec3::new(vx, vy, vz),
            sector,
            radius: RADIUS,
            damage: 100,
            owner: Shooter::Player,
            targets: Targets::All,
            life: 1.0,
            gravity: 12.0,
            bounce: Some(0.45),
            remote: true,
            splash: None,
            resting: false,
        };
        let bodies: [Body; 0] = [];
        for tick in 0..600 {
            let out = step_projectile(&map, &mut p, &bodies, |_| false, DT);
            prop_assert!(
                matches!(out, ProjectileStep::Flying | ProjectileStep::Resting),
                "tick {}: {:?}", tick, out
            );
            let s = map.find_sector(p.pos.truncate(), None);
            prop_assert!(s.is_some(), "tick {}: {:?} outside every sector", tick, p.pos);
            let sec = &map.sectors[s.unwrap()];
            prop_assert!(
                sec.floor_z - RADIUS - TOL <= p.pos.z && p.pos.z <= sec.ceil_z + RADIUS + TOL,
                "tick {}: {:?} outside z range of sector {:?}", tick, p.pos, s
            );
        }
    }
}
