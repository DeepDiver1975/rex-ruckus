//! A slider carrying a rider never leaves it outside the map or inside a wall, wherever the
//! rider stands and however often the slider is sent back and forth.

mod common;

use proptest::prelude::*;
use rr_core::collide::Body;
use rr_core::fixtures::slide_yard;
use rr_core::geom::closest_point_on_segment;
use rr_core::glam::Vec2;
use rr_core::mechanics::Mechanics;

const DT: f32 = 1.0 / 60.0;
/// How far a body may sit inside a solid wall (the collision skin and float slack).
const EPS: f32 = 2e-3;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    #[test]
    fn rider_stays_in_the_map_and_out_of_walls(
        x in 4.1f32..6.1,
        y in 1.0f32..3.0,
        rides in prop::collection::vec(1usize..300, 1..6),
    ) {
        let mut map = slide_yard("(kind: Slide(by: (7.8, 0.0)), speed: 2.0)", "");
        let mut mech = Mechanics::new(&mut map);
        let b = Body::spawn(&map, Vec2::new(x, y), 0.35, 1.8).unwrap();
        prop_assume!(b.sector == 2);
        let mut bodies = [b];
        for ticks in rides {
            mech.toggle(0);
            for _ in 0..ticks {
                mech.tick(&mut map, &mut bodies, DT);
                let b = bodies[0];
                let p = b.pos.truncate();
                prop_assert!(p.is_finite());
                prop_assert_eq!(map.find_sector(p, Some(b.sector)), Some(b.sector), "outside at {}", p);
                for w in map.walls.iter().filter(|w| w.next_sector.is_none()) {
                    let d = closest_point_on_segment(p, w.a, w.b).distance(p);
                    prop_assert!(d >= b.radius - EPS, "{} m from wall {:?}-{:?} at {}", d, w.a, w.b, p);
                }
            }
        }
    }
}
