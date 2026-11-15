//! Scripted level blasts (`Map::blasts`): a channel sets them off once, on their delay, and they
//! go off through the same queue as rockets and barrels.

use super::tests::{DT, Player, count, spawn_at};
use super::*;
use crate::fixtures::{defs, door_rooms};
use crate::mechanics::{Mechanics, Motion};

/// Room A x∈[0,4], a crack (sector 1) at x∈[4,4.5], room B x∈[4.5,8.5]. Blasts centred in B
/// at (7.5, 2, 1), 3 m from the crack's wall: channel 5 reaches it (3.5 m), channel 6 does not
/// (2.5 m); channel 7's goes off 0.5 s after the channel fires.
fn blast_rooms() -> (Map, Mechanics) {
    let mut map = door_rooms(
        "(kind: Crack)",
        "blasts: [
            (channel: 5, at: (7.5, 2.0, 1.0), radius: 3.5, damage: 40),
            (channel: 6, at: (7.5, 2.0, 1.0), radius: 2.5, damage: 40),
            (channel: 7, at: (7.5, 2.0, 1.0), radius: 3.5, damage: 40, delay: 0.5),
        ],",
    );
    let mech = Mechanics::new(&mut map);
    (map, mech)
}

fn booms(ev: &[CombatEvent]) -> usize {
    count(ev, |e| matches!(e, CombatEvent::Explosion { .. }))
}

fn cracks(ev: &[CombatEvent]) -> usize {
    count(ev, |e| matches!(e, CombatEvent::CrackOpened(_)))
}

#[test]
fn a_channel_sets_its_blast_off_once_after_the_delay() {
    let d = defs();
    let (mut map, mut mech) = blast_rooms();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    // Nothing goes off before the channel fires.
    assert_eq!(booms(&p.tick_with(&mut c, &mut map, &mut mech, &d)), 0);

    mech.fire(7);
    let mut log = Vec::new();
    let mut at = None;
    for t in 1..=60 {
        let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
        if booms(&ev) > 0 && at.is_none() {
            at = Some(t);
        }
        log.extend(ev);
    }
    let at = at.expect("the blast went off");
    assert!(
        (at as f32 * DT - 0.5).abs() <= 1.5 * DT,
        "0.5 s after the channel fired, not after {} s",
        at as f32 * DT
    );
    assert_eq!(booms(&log), 1);
    assert_eq!(cracks(&log), 1);
    assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    // The player in room A was behind the closed crack: unhurt.
    assert_eq!(p.vitals.health.hp, 100);

    // Firing the channel again does not re-arm it.
    mech.fire(7);
    let mut again = Vec::new();
    for _ in 0..60 {
        again.extend(p.tick_with(&mut c, &mut map, &mut mech, &d));
    }
    assert_eq!(booms(&again), 0, "{again:?}");
}

#[test]
fn a_scripted_blast_hurts_with_falloff_and_opens_cracks_in_reach_only() {
    let d = defs();
    let (mut map, _) = blast_rooms();
    // A sleeping Grunt (radius 0.35) 1.15 m from the centre's surface distance in B.
    spawn_at(&mut map, 6.0, 2.0, 0.0, true);
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    // The player in B, 0.85 m from the centre (surface distance of its 0.35 m cylinder).
    let mut p = Player::at(&map, 7.5, 3.2);

    // Channel 6: 2.5 m does not reach the crack's wall 3 m away.
    mech.fire(6);
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert_eq!(booms(&ev), 1);
    assert_eq!(cracks(&ev), 0, "out of reach: {ev:?}");
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
    // Linear falloff: 40 x (1 - 1.15 / 2.5) = 21.6 -> 22 for the Grunt; the player has no
    // owner's discount from a level blast: 40 x (1 - 0.85 / 2.5) = 26.4 -> 26.
    assert!(
        ev.contains(&CombatEvent::ActorHurt {
            actor: 0,
            amount: 22
        }),
        "{ev:?}"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, CombatEvent::PlayerHurt { amount: 26, .. })),
        "{ev:?}"
    );
    assert_eq!(p.vitals.health.hp, 74);
    // A level blast is nobody's: the Grunt does not turn on anyone for it.
    assert_eq!(c.actors[0].target, crate::actors::Target::Player);

    // Channel 5: 3.5 m reaches the crack.
    mech.fire(5);
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert_eq!(booms(&ev), 1);
    assert_eq!(cracks(&ev), 1, "{ev:?}");
    assert!(
        ev.contains(&CombatEvent::ActorKilled {
            actor: 0,
            by: Shooter::World
        }),
        "the second blast finishes the Grunt: {ev:?}"
    );
}

#[test]
fn a_blast_outside_every_sector_is_dropped() {
    let d = defs();
    let mut map = door_rooms(
        "(kind: Crack)",
        "blasts: [(channel: 5, at: (20.0, 2.0, 1.0), radius: 3.5, damage: 40)],",
    );
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    mech.fire(5);
    assert_eq!(booms(&p.tick_with(&mut c, &mut map, &mut mech, &d)), 0);
    assert!(c.pending_blasts.is_empty());
}
