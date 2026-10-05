mod common;

use proptest::prelude::*;
use rr_core::collide::{Body, z_range};
use rr_core::fixtures::lift_shaft;
use rr_core::glam::Vec2;
use rr_core::map::{KeySet, Map};
use rr_core::mechanics::{Mechanics, UseTarget};
use rr_core::movement::{MoveInput, Tuning, step_player};

const DT: f32 = 1.0 / 60.0;

/// (wish x, wish y, jump, crouch, ticks, toggle the lift first)
type Segment = (f32, f32, bool, bool, usize, bool);

fn segments() -> impl Strategy<Value = Vec<Segment>> {
    prop::collection::vec(
        (
            -1.0f32..1.0,
            -1.0f32..1.0,
            any::<bool>(),
            any::<bool>(),
            1usize..20,
            any::<bool>(),
        ),
        1..40,
    )
}

fn play(mut map: Map, start: Vec2, segs: &[Segment]) -> Result<(), TestCaseError> {
    let mut mech = Mechanics::new(&mut map);
    let t = Tuning::default();
    let mut b = Body::spawn(&map, start, 0.35, t.stand_height).unwrap();
    for &(x, y, jump, crouch, ticks, toggle) in segs {
        if toggle && !mech.movers.is_empty() {
            mech.activate(&map, UseTarget::Mover(0), KeySet::default());
        }
        for _ in 0..ticks {
            step_player(
                &map,
                &mut b,
                &MoveInput {
                    wish: Vec2::new(x, y),
                    jump,
                    crouch,
                },
                &t,
                DT,
            );
            let mut bodies = [b];
            mech.tick(&mut map, &mut bodies, DT);
            b = bodies[0];
            prop_assert!(b.pos.is_finite() && b.vel.is_finite(), "non-finite {:?}", b);
            prop_assert_eq!(
                map.find_sector(b.pos.truncate(), Some(b.sector)),
                Some(b.sector),
                "outside at {}",
                b.pos
            );
            let (floor, ceil) = z_range(&map, b.pos.truncate(), b.radius, b.sector);
            prop_assert!(
                b.pos.z >= floor - 1e-3,
                "feet {} below floor {}",
                b.pos.z,
                floor
            );
            prop_assert!(
                b.pos.z + b.height <= ceil + 1e-3,
                "head {} above ceiling {}",
                b.pos.z + b.height,
                ceil
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(64)))]

    #[test]
    fn random_play_in_test_yard(segs in segments()) {
        let map = Map::from_ron(include_str!("../../../assets/levels/test_yard.ron")).unwrap();
        let s = map.player_start.pos;
        play(map, Vec2::new(s.0, s.1), &segs)?;
    }

    #[test]
    fn random_play_on_a_lift(segs in segments()) {
        play(lift_shaft("(kind: Lift(to: 2.0))", ""), Vec2::new(5.0, 2.0), &segs)?;
    }
}
