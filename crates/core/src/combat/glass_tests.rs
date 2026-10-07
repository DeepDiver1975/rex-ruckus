//! Glass panes: shots, kicks, projectiles and blasts shatter them; nothing sees through one.

use super::tests::{Player, count, spawn_at, spawn_kind};
use super::*;
use crate::actors::volley;
use crate::defs::{SplashDef, WeaponId};
use crate::fixtures::{defs, glass_rooms};
use crate::health::PLAYER_MAX_HEALTH;
use crate::map::ActorKind;
use crate::weapons::WeaponEvent;
use std::f32::consts::PI;

/// Both sides of the pane in `glass_rooms`: (hall side, booth side).
fn pane(map: &Map) -> (usize, usize) {
    let w = (0..map.walls.len())
        .find(|&w| map.walls[w].glass && map.walls[w].sector == 0)
        .expect("glass_rooms has a pane");
    (w, map.walls[w].next_wall.unwrap())
}

fn glass_broken(ev: &[CombatEvent]) -> Vec<(usize, Vec<SectorId>)> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::GlassBroken { wall, dirty } => Some((*wall, dirty.clone())),
            _ => None,
        })
        .collect()
}

fn impacts(ev: &[CombatEvent]) -> Vec<Vec3> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::Impact { point, .. } => Some(*point),
            _ => None,
        })
        .collect()
}

#[test]
fn shot_breaks_glass_then_passes() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, back) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 8.0, 5.0);

    // The first shot shatters the pane and stops there: no impact behind it.
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert_eq!(glass_broken(&ev), vec![(w, vec![0, 1])], "{ev:?}");
    assert!(impacts(&ev).is_empty(), "{ev:?}");
    assert!(!map.walls[w].glass && !map.walls[back].glass);

    // The next one flies through the empty frame to the booth's far wall.
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![Vec3::X]);
    assert!(glass_broken(&ev).is_empty());
    let hit = impacts(&ev);
    assert_eq!(hit.len(), 1, "{ev:?}");
    assert!((hit[0].x - 14.0).abs() < 1e-3, "{hit:?}");
}

#[test]
fn shotgun_volley_breaks_the_pane_once() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 8.0, 5.0);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Shotgun, vec![Vec3::X; 4]);
    // The first pellet breaks it; the rest fly on into the booth.
    assert_eq!(glass_broken(&ev), vec![(w, vec![0, 1])]);
    assert_eq!(impacts(&ev).len(), 3, "{ev:?}");
}

#[test]
fn kick_breaks_glass() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 9.2, 5.0);
    let kick = WeaponEvent::Kick { dir: Vec3::X };
    let ev = c.player_attack(&mut map, &d, p.eye(), p.body.sector, &kick);
    assert_eq!(glass_broken(&ev), vec![(w, vec![0, 1])], "{ev:?}");
}

#[test]
fn rocket_breaks_glass_and_explodes_at_it() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 3.0, 8.0);
    let aim = (Vec3::new(10.0, 5.0, 1.6) - p.eye()).normalize();
    let ev = WeaponEvent::Launch {
        weapon: WeaponId::Rockets,
        dir: aim,
    };
    c.player_attack(&mut map, &d, p.eye(), p.body.sector, &ev);
    let mut log = Vec::new();
    for _ in 0..120 {
        log.extend(p.tick(&mut c, &mut map, &d));
        if count(&log, |e| matches!(e, CombatEvent::Explosion { .. })) > 0 {
            break;
        }
    }
    assert_eq!(glass_broken(&log), vec![(w, vec![0, 1])], "{log:?}");
    assert!(c.projectiles.is_empty(), "the rocket is spent");
    let boom = log
        .iter()
        .find_map(|e| match e {
            CombatEvent::Explosion { point, .. } => Some(*point),
            _ => None,
        })
        .expect("the rocket still goes off");
    assert!(boom.x < 10.0 && boom.x > 9.8, "just off the pane: {boom:?}");
}

#[test]
fn blast_shatters_glass_in_reach_but_glass_shields_this_blast() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    spawn_at(&mut map, 11.0, 5.0, 0.0, true); // in the booth, right behind the pane
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 2.0, 1.0);
    c.pending_blasts.push(PendingBlast {
        fuse: 0.0,
        via: Via::Direct,
        blast: Blast {
            center: Vec3::new(9.0, 5.0, 1.0),
            sector: 0,
            splash: SplashDef {
                radius: 4.0,
                damage: 100,
                self_scale: 0.5,
            },
            owner: Shooter::Player,
        },
    });
    let ev = p.tick(&mut c, &mut map, &d);
    assert_eq!(glass_broken(&ev), vec![(w, vec![0, 1])], "{ev:?}");
    assert!(!map.walls[w].glass);
    assert_eq!(
        count(&ev, |e| matches!(e, CombatEvent::ActorHurt { .. })),
        0,
        "the pane took this blast: {ev:?}"
    );
    // Its bang then carries through the empty frame.
    assert!(ev.contains(&CombatEvent::ActorWoke(0)), "{ev:?}");
}

#[test]
fn enforcer_blocked_by_glass() {
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    spawn_kind(&mut map, ActorKind::Enforcer, 12.5, 5.0, PI, false);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 7);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 5.0);
    for _ in 0..600 {
        let ev = p.tick(&mut c, &mut map, &d);
        assert_eq!(
            count(&ev, |e| matches!(
                e,
                CombatEvent::ActorFired { .. } | CombatEvent::PlayerHurt { .. }
            )),
            0,
            "{ev:?}"
        );
    }
    assert_eq!(p.vitals.health.hp, PLAYER_MAX_HEALTH);
    assert!(map.walls[w].glass, "the pane is intact");
    assert_eq!(c.actors[0].body.sector, 1, "no way out of the booth");

    // With the pane gone the Enforcer sees, fires and hurts.
    let mut d2 = defs();
    for e in &mut d2.enemies {
        if e.kind == ActorKind::Enforcer {
            e.strafe = false;
        }
    }
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    spawn_kind(&mut map, ActorKind::Enforcer, 12.5, 5.0, PI, false);
    let mut c = Combat::spawn(&map, &d2, 7);
    c.actors[0].state = AiState::Chase;
    c.destruct.break_glass(&mut map, w);
    let mut p = Player::at(&map, 7.0, 5.0);
    let mut fired = 0;
    for _ in 0..600 {
        let ev = p.tick(&mut c, &mut map, &d2);
        fired += count(&ev, |e| matches!(e, CombatEvent::ActorFired { .. }));
    }
    assert!(fired > 0);
    assert!(p.vitals.health.hp < PLAYER_MAX_HEALTH);
}

#[test]
fn enemy_hitscan_volley_reports_glass_once() {
    let d = defs();
    let mut map = glass_rooms();
    let (_, back) = pane(&map);
    spawn_kind(&mut map, ActorKind::Enforcer, 12.5, 5.0, PI, false);
    let mut c = Combat::spawn(&map, &d, 7);
    let a = c.actors[0].clone();
    let bodies = vec![
        Body::spawn(&map, Vec2::new(2.0, 1.0), 0.35, 1.8).unwrap(),
        a.body,
    ];
    let v = volley(
        &map,
        &a,
        0,
        d.enemy(ActorKind::Enforcer),
        Vec3::NEG_X,
        &bodies,
        |_| true,
        &mut c.rng,
    );
    // Every pellet stops at the pane, which is reported once (from the booth side).
    assert_eq!(v.glass, vec![back]);
    assert!(v.impacts.is_empty(), "{v:?}");
    assert_eq!(v.player_damage, 0);
}

#[test]
fn shot_above_pane_does_not_break_it() {
    let d = defs();
    let mut map = glass_rooms();
    let (w, _) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 8.0, 5.0);
    // The hall is 4 m tall, the booth 3 m: the strip above the pane is solid wall.
    let dir = (Vec3::new(10.0, 5.0, 3.5) - p.eye()).normalize();
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![dir]);
    assert!(glass_broken(&ev).is_empty(), "{ev:?}");
    let hit = impacts(&ev);
    assert_eq!(hit.len(), 1, "{ev:?}");
    assert!((hit[0].x - 10.0).abs() < 1e-3 && (hit[0].z - 3.5).abs() < 1e-3);
    assert!(map.walls[w].glass, "the pane is intact");
}

#[test]
fn rocket_into_sill_does_not_break_pane() {
    let mut d = defs();
    // A small splash, so the blast off the sill cannot reach the pane above it either.
    for wd in &mut d.weapons.weapons {
        if let crate::defs::Attack::Projectile { proj } = &mut wd.attack
            && let Some(s) = &mut proj.splash
        {
            s.radius = 0.3;
        }
    }
    let mut map = glass_rooms();
    map.sectors[1].floor_z = 1.0; // a 1 m sill under the pane
    let (w, _) = pane(&map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 8.0, 5.0);
    let aim = (Vec3::new(10.0, 5.0, 0.4) - p.eye()).normalize();
    let ev = WeaponEvent::Launch {
        weapon: WeaponId::Rockets,
        dir: aim,
    };
    c.player_attack(&mut map, &d, p.eye(), p.body.sector, &ev);
    let mut log = Vec::new();
    for _ in 0..60 {
        log.extend(p.tick(&mut c, &mut map, &d));
        if count(&log, |e| matches!(e, CombatEvent::Explosion { .. })) > 0 {
            break;
        }
    }
    assert!(glass_broken(&log).is_empty(), "{log:?}");
    let hit = impacts(&log);
    assert_eq!(hit.len(), 1, "an ordinary wall hit: {log:?}");
    assert!(hit[0].z < 1.0, "{hit:?}");
    assert_eq!(
        count(&log, |e| matches!(e, CombatEvent::Explosion { .. })),
        1
    );
    assert!(c.projectiles.is_empty(), "the rocket is spent");
    assert!(map.walls[w].glass, "the pane is intact");
}

#[test]
fn enemy_volley_into_soffit_does_not_report_glass() {
    let d = defs();
    let mut map = glass_rooms();
    spawn_kind(&mut map, ActorKind::Enforcer, 8.0, 5.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 7);
    let a = c.actors[0].clone();
    let bodies = vec![
        Body::spawn(&map, Vec2::new(2.0, 1.0), 0.35, 1.8).unwrap(),
        a.body,
    ];
    let dir = (Vec3::new(10.0, 5.0, 3.6) - a.muzzle()).normalize();
    let v = volley(
        &map,
        &a,
        0,
        d.enemy(ActorKind::Enforcer),
        dir,
        &bodies,
        |_| true,
        &mut c.rng,
    );
    assert!(v.glass.is_empty(), "{v:?}");
    assert!(!v.impacts.is_empty());
    assert!(v.impacts.iter().all(|h| h.point.z > 3.0), "{v:?}");
}
