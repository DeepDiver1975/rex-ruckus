//! Breakable light fixtures: shots and blasts break them once, walls shield them.

use super::tests::Player;
use super::*;
use crate::defs::{SplashDef, WeaponId};
use crate::explosion::Blast;
use crate::fixtures::{defs, glass_rooms};
use crate::map::RawLight;
use crate::projectile::Shooter;

fn light(pos: (f32, f32, f32), breakable: bool) -> RawLight {
    RawLight {
        pos,
        color: (1.0, 1.0, 1.0),
        intensity: 1000.0,
        range: 8.0,
        breakable,
    }
}

fn broken(ev: &[CombatEvent]) -> Vec<usize> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::LightBroken(i) => Some(*i),
            _ => None,
        })
        .collect()
}

fn impacts(ev: &[CombatEvent]) -> usize {
    ev.iter()
        .filter(|e| matches!(e, CombatEvent::Impact { .. }))
        .count()
}

/// The hall of `glass_rooms` with the player at (8,5), eye 1.6 m, aiming east at the pane.
fn setup(lights: Vec<RawLight>) -> (Map, Combat, Player) {
    let mut map = glass_rooms();
    map.lights = lights;
    let c = Combat::spawn(&map, &defs(), 1);
    let p = Player::at(&map, 8.0, 5.0);
    (map, c, p)
}

#[test]
fn shot_breaks_light() {
    let d = defs();
    let (mut map, mut c, p) = setup(vec![
        light((9.0, 5.0, 1.6), true),
        light((9.0, 7.0, 1.6), true),
    ]);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert_eq!(broken(&ev), vec![0], "{ev:?}");
    // The pellet stops at the fixture: no impact on the pane behind it.
    assert_eq!(impacts(&ev), 0, "{ev:?}");
    assert!(map.walls.iter().any(|w| w.glass));
    // The second light (above and to the side) is not on that line and stays intact.
    let ev = p.shoot(
        &mut c,
        &mut map,
        &d,
        WeaponId::Pistol,
        vec![Vec3::new(1.0, 0.0, 0.0)],
    );
    assert!(broken(&ev).is_empty(), "{ev:?}");
}

#[test]
fn shot_off_the_fixture_misses_it() {
    let d = defs();
    let (mut map, mut c, p) = setup(vec![light((9.0, 5.3, 1.6), true)]);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(broken(&ev).is_empty(), "{ev:?}");
}

#[test]
fn unbreakable_light_is_not_hit() {
    let d = defs();
    let (mut map, mut c, p) = setup(vec![light((9.0, 5.0, 1.6), false)]);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(broken(&ev).is_empty(), "{ev:?}");
    assert!(!c.destruct.break_light(&map, 0));
}

#[test]
fn light_behind_wall_not_hit() {
    let d = defs();
    // Behind the glass pane (x = 10) in the booth: the pane is hit first.
    let (mut map, mut c, p) = setup(vec![light((12.0, 5.0, 1.6), true)]);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(broken(&ev).is_empty(), "{ev:?}");
    // Behind the hall's solid west wall.
    let (mut map, mut c, p) = setup(vec![light((-1.0, 5.0, 1.6), true)]);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![-Vec3::X]);
    assert!(broken(&ev).is_empty(), "{ev:?}");
    assert_eq!(impacts(&ev), 1, "{ev:?}");
}

#[test]
fn kick_breaks_light_in_reach() {
    let d = defs();
    let (mut map, mut c, p) = setup(vec![light((8.5, 5.0, 1.6), true)]);
    let ev = c.player_attack(
        &mut map,
        &d,
        p.eye(),
        p.body.sector,
        &crate::weapons::WeaponEvent::Kick { dir: Vec3::X },
    );
    assert_eq!(broken(&ev), vec![0], "{ev:?}");
}

fn blast(map: &Map, center: Vec3, radius: f32) -> Blast {
    Blast {
        center,
        sector: map.find_sector(center.truncate(), None).unwrap(),
        splash: SplashDef {
            radius,
            damage: 10,
            self_scale: 0.5,
        },
        owner: Shooter::Player,
    }
}

fn explode(c: &mut Combat, map: &mut Map, b: Blast) -> Vec<CombatEvent> {
    let d = defs();
    let mut p = Player::at(map, 1.0, 9.0);
    c.queue_blast(0.0, b);
    p.tick(c, map, &d)
}

#[test]
fn blast_breaks_light() {
    let (mut map, mut c, _) = setup(vec![
        light((5.0, 2.0, 2.5), true),  // in reach
        light((5.0, 2.0, 3.5), false), // not breakable
        light((9.0, 9.0, 2.5), true),  // out of reach
        light((2.0, 2.0, 2.5), true),  // out of reach
    ]);
    let b = blast(&map, Vec3::new(5.0, 3.0, 1.0), 3.0);
    let ev = explode(&mut c, &mut map, b);
    assert_eq!(broken(&ev), vec![0], "{ev:?}");
}

#[test]
fn blast_light_needs_line_of_sight() {
    // The pillar (x,y in 4..6) hides a light on its far side from a blast on this side.
    let (mut map, mut c, _) = setup(vec![light((5.0, 6.8, 1.5), true)]);
    let b = blast(&map, Vec3::new(5.0, 3.2, 1.5), 4.0);
    let ev = explode(&mut c, &mut map, b);
    assert!(broken(&ev).is_empty(), "{ev:?}");
}

#[test]
fn broken_light_stays_broken() {
    let d = defs();
    let (mut map, mut c, p) = setup(vec![light((9.0, 5.0, 1.6), true)]);
    assert!(c.destruct.break_light(&map, 0));
    assert!(!c.destruct.break_light(&map, 0));
    assert!(!c.destruct.break_light(&map, 7));
    // A broken fixture is no longer in the way: the pellet carries on to the pane.
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(broken(&ev).is_empty(), "{ev:?}");
    // Nor does a blast report it again.
    let b = blast(&map, Vec3::new(8.0, 5.0, 1.6), 3.0);
    let ev = explode(&mut c, &mut map, b);
    assert!(broken(&ev).is_empty(), "{ev:?}");
}
