//! Crack walls and secrets: only blasts open a crack; each secret is found once.

use super::tests::{Player, count};
use super::*;
use crate::defs::{SplashDef, WeaponId};
use crate::explosion::Blast;
use crate::fixtures::{defs, door_rooms};
use crate::interact::use_target;
use crate::mechanics::{Motion, UseOutcome, UseTarget};
use crate::projectile::Shooter;

const BANG: SplashDef = SplashDef {
    radius: 6.0,
    damage: 50,
    self_scale: 0.5,
};

/// Room A, a crack at x∈[4,4.5] (sector 1, open height 3), room B.
fn crack_map(extra: &str) -> (Map, Mechanics) {
    let mut map = door_rooms("(kind: Crack)", extra);
    let mech = Mechanics::new(&mut map);
    (map, mech)
}

fn blast_at(map: &Map, c: Vec3) -> Blast {
    Blast {
        center: c,
        sector: map.find_sector(c.truncate(), None).unwrap(),
        splash: BANG,
        owner: Shooter::Player,
    }
}

fn cracks_opened(ev: &[CombatEvent]) -> Vec<SectorId> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::CrackOpened(s) => Some(*s),
            _ => None,
        })
        .collect()
}

#[test]
fn crack_ignores_use_and_shots() {
    let d = defs();
    let (mut map, mut mech) = crack_map("");
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 3.0, 2.0);
    // The use key never offers it, and pressing use on it does nothing.
    assert_eq!(use_target(&map, &mech, &p.body, 0.0), None);
    assert_eq!(
        mech.activate(&map, UseTarget::Mover(0), Default::default()),
        UseOutcome::Activated
    );
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
    // A shot at the wall leaves it shut.
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(cracks_opened(&ev).is_empty());
    p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
    assert_eq!(map.sectors[1].ceil_z, 0.0);

    // Nor does a channel, even one the crack listens on.
    let mut map = door_rooms("(kind: Crack, channel: Some(1))", "");
    let mut mech = Mechanics::new(&mut map);
    assert_eq!(mech.fire(1), 0);
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
}

#[test]
fn crack_opens_to_explosion_in_range() {
    let d = defs();
    let (mut map, mut mech) = crack_map("");
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 0.5, 0.5);
    c.queue_blast(0.0, blast_at(&map, Vec3::new(2.0, 2.0, 1.0)));
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert_eq!(cracks_opened(&ev), vec![1], "{ev:?}");
    assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    // It opens through the normal mechanics tick, fast.
    let dirty = mech.tick(&mut map, &mut [], 1.0 / 60.0);
    assert_eq!(dirty, vec![1]);
    for _ in 0..40 {
        mech.tick(&mut map, &mut [], 1.0 / 60.0);
    }
    assert_eq!(map.sectors[1].ceil_z, 3.0);
}

#[test]
fn crack_out_of_reach_stays_shut() {
    let d = defs();
    let (mut map, mut mech) = crack_map("");
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 0.5, 0.5);
    // 3 m from the crack's wall, with a 2 m radius.
    let mut b = blast_at(&map, Vec3::new(1.0, 2.0, 1.0));
    b.splash.radius = 2.0;
    c.queue_blast(0.0, b);
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert!(cracks_opened(&ev).is_empty(), "{ev:?}");
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
}

#[test]
fn crack_stays_open() {
    let d = defs();
    let (mut map, mut mech) = crack_map("");
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 0.5, 0.5);
    c.queue_blast(0.0, blast_at(&map, Vec3::new(2.0, 2.0, 1.0)));
    p.tick_with(&mut c, &mut map, &mut mech, &d);
    for _ in 0..300 {
        mech.tick(&mut map, &mut [], 1.0 / 60.0);
    }
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
    // It never closes: no auto-return, no use, no channel.
    mech.activate(&map, UseTarget::Mover(0), Default::default());
    mech.fire(1);
    for _ in 0..600 {
        mech.tick(&mut map, &mut [], 1.0 / 60.0);
    }
    assert_eq!(map.sectors[1].ceil_z, 3.0);
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
    // A second blast neither re-reports nor restarts it.
    c.queue_blast(0.0, blast_at(&map, Vec3::new(2.0, 2.0, 1.0)));
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert!(cracks_opened(&ev).is_empty(), "{ev:?}");
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
}

#[test]
fn secret_counted_once() {
    let d = defs();
    let (mut map, mut mech) = crack_map("");
    map.sectors[2].secret = true;
    let mut c = Combat::spawn(&map, &d, 1);
    assert_eq!(c.destruct.secrets(), (0, 1));
    let mut p = Player::at(&map, 2.0, 2.0);
    let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
    assert_eq!(count(&ev, |e| matches!(e, CombatEvent::SecretFound)), 0);
    // Step into the secret sector: found once, however long the player stays or returns.
    p.body = Body::spawn(&map, glam::Vec2::new(6.5, 2.0), 0.35, 1.8).unwrap();
    assert_eq!(p.body.sector, 2);
    let mut found = 0;
    for _ in 0..3 {
        let ev = p.tick_with(&mut c, &mut map, &mut mech, &d);
        found += count(&ev, |e| matches!(e, CombatEvent::SecretFound));
    }
    assert_eq!(found, 1);
    assert_eq!(c.destruct.secrets(), (1, 1));
    assert!(!c.destruct.enter_sector(&map, 2));
    assert!(!c.destruct.enter_sector(&map, 0));
}
