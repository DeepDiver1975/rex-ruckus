use proptest::prelude::*;
use rr_core::collide::{Body, clip_move};
use rr_core::fixtures::{pillar_room, two_rooms};
use rr_core::geom::closest_point_on_segment;
use rr_core::glam::Vec2;
use rr_core::map::Map;

const R: f32 = 0.35;

fn check_walk(map: &Map, start: Vec2, steps: &[(f32, f32)]) -> Result<(), TestCaseError> {
    let mut b = Body::spawn(map, start, R, 1.8).unwrap();
    for &(dx, dy) in steps {
        clip_move(map, &mut b, Vec2::new(dx, dy), 0.55);
        let p = b.pos.truncate();
        prop_assert_eq!(
            map.find_sector(p, Some(b.sector)),
            Some(b.sector),
            "outside at {}",
            p
        );
        for w in map.walls.iter().filter(|w| w.next_sector.is_none()) {
            let d = closest_point_on_segment(p, w.a, w.b).distance(p);
            prop_assert!(
                d >= R - 0.01,
                "penetrating wall {:?} at {} (d = {})",
                w,
                p,
                d
            );
        }
    }
    Ok(())
}

proptest! {
    #[test]
    fn random_walks_stay_inside_pillar_room(steps in prop::collection::vec((-3.0f32..3.0, -3.0f32..3.0), 1..60)) {
        check_walk(&pillar_room(), Vec2::new(2.0, 2.0), &steps)?;
    }

    #[test]
    fn random_walks_stay_inside_two_rooms(steps in prop::collection::vec((-3.0f32..3.0, -3.0f32..3.0), 1..60)) {
        check_walk(&two_rooms(0.4, 3.0), Vec2::new(2.0, 2.0), &steps)?;
    }
}
