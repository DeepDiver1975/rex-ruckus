//! Damaging floors through the combat tick.

use super::tests::{Player, count, mech_tick};
use super::*;
use crate::fixtures::{defs, engine_room, lift_shaft};
use crate::hazard::{Hazard, HazardKind};

fn burns(ev: &[CombatEvent]) -> usize {
    count(ev, |e| {
        matches!(
            e,
            CombatEvent::HazardBurn {
                kind: HazardKind::Slime
            }
        )
    })
}

#[test]
fn standing_in_slime_burns_through_vitals() {
    let mut map = engine_room("");
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 11.0, 2.0);
    let mut n = 0;
    for _ in 0..60 {
        let ev = p.tick(&mut c, &mut map, &d);
        n += burns(&ev);
        if let Some(i) = ev
            .iter()
            .position(|e| matches!(e, CombatEvent::HazardBurn { .. }))
        {
            assert!(matches!(
                ev[i + 1],
                CombatEvent::PlayerHurt { amount: 4, .. }
            ));
        }
    }
    assert_eq!(n, 2, "at once and after 0.75 s");
    assert_eq!(p.vitals.health.hp, 92);
}

#[test]
fn jetpack_flight_over_slime_is_safe() {
    let mut map = engine_room("");
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 11.0, 2.0);
    p.body.on_ground = false;
    p.body.pos.z += 1.0;
    for _ in 0..60 {
        assert_eq!(burns(&p.tick(&mut c, &mut map, &d)), 0);
    }
}

#[test]
fn slime_on_a_rising_lift_keeps_burning_on_schedule() {
    // Contact follows the live floor of a moving sector.
    let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
    map.sectors[1].hazard = Some(Hazard {
        damage: 4,
        interval: 0.75,
        kind: HazardKind::Slime,
    });
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut mech = Mechanics::new(&mut map);
    let mut p = Player::at(&map, 5.0, 2.0);
    mech.toggle(0);
    let mut n = 0;
    for _ in 0..150 {
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
        n += burns(&p.tick_with(&mut c, &mut map, &mut mech, &d));
    }
    assert_eq!(p.body.pos.z, 2.0, "rode to the top");
    assert_eq!(n, 4, "0, 0.75, 1.5 and 2.25 s");
}
