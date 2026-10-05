mod common;

use proptest::prelude::*;
use rr_core::collide::{Body, z_range};
use rr_core::combat::{Combat, PlayerTarget, level_seed};
use rr_core::defs::Defs;
use rr_core::fixtures::{combat_room, glass_rooms, pillar_room, two_rooms};
use rr_core::glam::{Vec2, Vec3};
use rr_core::map::{ActorKind, ActorSpawn, Map};
use rr_core::mechanics::Mechanics;
use rr_core::vitals::Vitals;

const DT: f32 = 1.0 / 60.0;
/// Tolerance on the z bounds (float noise only).
const EPS: f32 = 1e-3;

fn fixture(i: usize) -> Map {
    match i {
        0 => pillar_room(),
        1 => two_rooms(1.0, 3.0),
        2 => two_rooms(0.4, 0.9),
        3 => glass_rooms(),
        _ => combat_room(),
    }
}

/// A point inside the map's bounding box, as fractions of its extent.
fn point(map: &Map, fx: f32, fy: f32) -> Vec2 {
    let (lo, hi) = map.walls.iter().map(|w| w.a).fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), v| (lo.min(v), hi.max(v)),
    );
    lo + (hi - lo) * Vec2::new(fx, fy)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    /// A living Drone, hunted around by a wandering player, always stays between its floor
    /// clearance and the ceiling (where the range is not empty), and never leaves the map.
    #[test]
    fn drone_stays_between_floor_and_ceiling(
        map_id in 0usize..5,
        start in (0.0f32..1.0, 0.0f32..1.0),
        players in prop::collection::vec((0.0f32..1.0, 0.0f32..1.0), 1..6),
        seed in any::<u64>(),
    ) {
        let mut map = fixture(map_id);
        let at = point(&map, start.0, start.1);
        prop_assume!(map.find_sector(at, None).is_some());
        map.actors.push(ActorSpawn { kind: ActorKind::Drone, pos: at, angle: 0.0, asleep: false });
        let defs = Defs::builtin();
        let mut combat = Combat::spawn(&map, &defs, level_seed(&map.name) ^ seed);
        prop_assume!(combat.actors.len() == 1);
        let mut mech = Mechanics::new(&mut map.clone());
        for (fx, fy) in players {
            let p = point(&map, fx, fy);
            let Some(mut body) = Body::spawn(&map, p, 0.35, 1.8) else { continue };
            for _ in 0..80 {
                let eye = body.pos + Vec3::Z * 1.6;
                let mut vitals = Vitals::new(); // keep the player alive: the drone keeps hunting
                let mut target = PlayerTarget { body: &mut body, vitals: &mut vitals, eye };
                combat.tick(&mut map, &mut mech, &defs, &mut target, DT);
                let b = combat.actors[0].body;
                let (floor, ceil) = z_range(&map, b.pos.truncate(), b.radius, b.sector);
                prop_assert!(b.pos.z >= floor - EPS, "below the floor: {} < {}", b.pos.z, floor);
                if ceil - floor >= b.height + 0.3 {
                    prop_assert!(b.pos.z >= floor + 0.3 - EPS, "under the clearance: {}", b.pos.z);
                }
                prop_assert!(
                    b.pos.z + b.height <= ceil + EPS || ceil - floor < b.height,
                    "head in the ceiling: {} + {} > {}", b.pos.z, b.height, ceil
                );
                prop_assert!(map.find_sector(b.pos.truncate(), None).is_some());
            }
        }
    }
}
