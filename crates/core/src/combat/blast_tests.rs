//! Player projectiles, the explosion queue and barrel chains.

use super::tests::{DT, Player, count, mech_tick, spawn_at, spawn_kind};
use super::*;
use crate::defs::{AmmoKind, Attack, SplashDef, WeaponId};
use crate::explosion::{Blast, solve};
use crate::fixtures::{combat_room, defs, lift_shaft};
use crate::health::PLAYER_MAX_HEALTH;
use crate::map::ActorKind;
use crate::mechanics::{Mechanics, Motion};
use crate::weapons::WeaponEvent;
use crate::weapons::{Arsenal, WeaponInput, WeaponPhase};
use std::f32::consts::PI;

impl Player {
    fn launch(
        &self,
        c: &mut Combat,
        map: &Map,
        d: &Defs,
        weapon: WeaponId,
        dir: Vec3,
    ) -> Vec<CombatEvent> {
        let ev = WeaponEvent::Launch { weapon, dir };
        c.player_attack(map, d, self.eye(), self.body.sector, &ev)
    }

    /// Ticks up to `n` times, stopping after the first tick that reports an `Explosion`.
    fn tick_until_explosion(
        &mut self,
        c: &mut Combat,
        map: &Map,
        d: &Defs,
        n: usize,
    ) -> Vec<CombatEvent> {
        let mut log = Vec::new();
        for _ in 0..n {
            let ev = self.tick(c, map, d);
            let boom = ev
                .iter()
                .any(|e| matches!(e, CombatEvent::Explosion { .. }));
            log.extend(ev);
            if boom {
                break;
            }
        }
        log
    }
}

fn explosions(ev: &[CombatEvent]) -> Vec<Vec3> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::Explosion { point, .. } => Some(*point),
            _ => None,
        })
        .collect()
}

fn hurt_total(ev: &[CombatEvent], actor: usize) -> i32 {
    ev.iter()
        .map(|e| match e {
            CombatEvent::ActorHurt { actor: a, amount } if *a == actor => *amount,
            _ => 0,
        })
        .sum()
}

fn proj_def(d: &Defs, w: WeaponId) -> ProjectileDef {
    match d.weapon(w).attack {
        Attack::Projectile { proj } => proj,
        a => panic!("{w:?} has no projectile: {a:?}"),
    }
}

fn rocket_splash(d: &Defs) -> SplashDef {
    proj_def(d, WeaponId::Rockets)
        .splash
        .expect("rockets splash")
}

/// Shipped defs with a Grunt that soaks `hp` damage.
fn tough_grunts(hp: i32) -> Defs {
    let mut d = defs();
    for e in &mut d.enemies {
        if e.kind == ActorKind::Grunt {
            e.health = hp;
        }
    }
    d
}

#[test]
fn player_rocket_hurts_grunt() {
    let d = defs();
    let mut map = combat_room();
    // Asleep and facing away, so neither ever shoots back.
    spawn_at(&mut map, 7.5, 1.5, 0.0, true);
    spawn_at(&mut map, 7.5, 2.6, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    let ev = p.launch(&mut c, &map, &d, WeaponId::Rockets, dir);
    assert_eq!(c.projectiles.len(), 1, "{ev:?}");
    let r = &c.projectiles[0];
    assert_eq!(r.owner, Shooter::Player);
    assert_eq!(r.targets, Targets::All);
    assert_eq!(r.pos, p.eye(), "launched from the eye");
    let pd = proj_def(&d, WeaponId::Rockets);
    assert!(
        (r.vel - dir * pd.speed).length() < 1e-4,
        "straight along the aim"
    );
    assert_eq!(r.splash, pd.splash);
    // The launch is loud: both sleepers wake.
    assert!(ev.contains(&CombatEvent::ActorWoke(0)), "{ev:?}");

    let log = p.tick_until_explosion(&mut c, &map, &d, 60);
    assert_eq!(hurt_total(&log, 0), pd.damage, "direct hit: {log:?}");
    assert!(
        hurt_total(&log, 1) > 0,
        "the neighbour takes splash: {log:?}"
    );
    let booms = explosions(&log);
    assert_eq!(booms.len(), 1);
    assert!(booms[0].truncate().distance(Vec2::new(7.5, 1.5)) < 0.6);
    assert!(log.contains(&CombatEvent::Explosion {
        point: booms[0],
        radius: rocket_splash(&d).radius,
    }));
    assert!(log.contains(&CombatEvent::ProjectileGone(0)));
    assert!(c.projectiles.is_empty());
    assert_eq!(
        p.vitals.health.hp, PLAYER_MAX_HEALTH,
        "beyond the 5 m splash"
    );
}

#[test]
fn direct_hit_not_double_counted() {
    let d = tough_grunts(1000);
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    p.launch(&mut c, &map, &d, WeaponId::Rockets, dir);
    let log = p.tick_until_explosion(&mut c, &map, &d, 60);
    let direct = proj_def(&d, WeaponId::Rockets).damage;
    assert_eq!(hurt_total(&log, 0), direct, "{log:?}");
    assert_eq!(c.actors[0].health.hp, 1000 - direct);
    // Without the exclusion the same blast would have hit it again.
    let center = explosions(&log)[0];
    let blast = Blast {
        center,
        sector: 0,
        splash: rocket_splash(&d),
        owner: Shooter::Player,
    };
    let again = solve(&map, &blast, &[p.body, c.actors[0].body], |i| i == 0);
    assert!(!again.is_empty(), "the grunt is inside the splash");
    // Later ticks add nothing.
    for _ in 0..30 {
        let ev = p.tick(&mut c, &map, &d);
        assert_eq!(hurt_total(&ev, 0), 0);
    }
}

#[test]
fn rocket_explodes_off_the_wall() {
    let d = defs();
    let map = combat_room();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    p.vitals.armour = 50;
    p.launch(&mut c, &map, &d, WeaponId::Rockets, Vec3::NEG_Y);
    let log = p.tick_until_explosion(&mut c, &map, &d, 60);
    let impact = log
        .iter()
        .find_map(|e| match e {
            CombatEvent::Impact {
                point,
                normal,
                sector,
            } => Some((*point, *normal, *sector)),
            _ => None,
        })
        .expect("the rocket hit the south wall");
    assert_eq!(impact.2, 0);
    assert!((impact.1 - Vec3::Y).length() < 1e-4);
    let booms = explosions(&log);
    assert_eq!(booms.len(), 1);
    // Nudged 5 cm off the wall along the normal.
    assert!((booms[0] - (impact.0 + Vec3::Y * 0.05)).length() < 1e-4);
    // Own splash, scaled, and routed through armour.
    let (amount, from) = log
        .iter()
        .find_map(|e| match e {
            CombatEvent::PlayerHurt { amount, from } => Some((*amount, *from)),
            _ => None,
        })
        .expect("the player is inside the splash");
    assert_eq!(from, booms[0]);
    let lost = PLAYER_MAX_HEALTH - p.vitals.health.hp;
    assert!(p.vitals.armour < 50, "armour soaked some");
    assert_eq!(lost + (50 - p.vitals.armour), amount);
}

#[test]
fn rocket_expiring_in_flight_explodes() {
    let d = defs();
    let map = combat_room();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    p.launch(&mut c, &map, &d, WeaponId::Rockets, Vec3::X);
    c.projectiles[0].life = 0.05;
    let log = p.tick_until_explosion(&mut c, &map, &d, 10);
    let booms = explosions(&log);
    assert_eq!(booms.len(), 1, "{log:?}");
    assert!(booms[0].x > 2.5 && booms[0].x < 4.0, "{booms:?}");
    assert!(c.projectiles.is_empty());
}

#[test]
fn bomb_is_lobbed_and_bounces_off_actors() {
    let d = tough_grunts(1000);
    let mut map = combat_room();
    spawn_at(&mut map, 3.0, 1.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let aim = Vec3::new(1.0, 0.0, -0.3).normalize();
    p.launch(&mut c, &map, &d, WeaponId::PipeBombs, aim);
    let pd = proj_def(&d, WeaponId::PipeBombs);
    let b = &c.projectiles[0];
    let want = (aim + Vec3::Z * 0.15).normalize() * pd.speed;
    assert!((b.vel - want).length() < 1e-4, "{:?} vs {want:?}", b.vel);
    assert!(b.remote && b.bounce.is_some());
    assert_eq!(c.live_bombs(), 1);
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(p.tick(&mut c, &map, &d));
    }
    assert_eq!(hurt_total(&log, 0), 0, "a bomb never hurts on contact");
    assert!(explosions(&log).is_empty());
    assert_eq!(c.projectiles.len(), 1);
    assert!(c.projectiles[0].pos.x < 3.0, "bounced back off the grunt");
    assert_eq!(c.live_bombs(), 1);
}

#[test]
fn detonate_blows_every_live_bomb() {
    let d = defs();
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Barrel, 7.0, 6.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    p.launch(&mut c, &map, &d, WeaponId::PipeBombs, Vec3::X);
    p.launch(&mut c, &map, &d, WeaponId::PipeBombs, Vec3::Y);
    for _ in 0..180 {
        p.tick(&mut c, &map, &d);
    }
    assert!(c.projectiles.iter().all(|b| b.resting), "both settled");
    assert_eq!(c.live_bombs(), 2);
    let at: Vec<Vec3> = c.projectiles.iter().map(|b| b.pos).collect();
    let ev = c.player_attack(&map, &d, p.eye(), p.body.sector, &WeaponEvent::Detonate);
    assert_eq!(count(&ev, |e| *e == CombatEvent::BombsDetonated), 1);
    assert!(ev.contains(&CombatEvent::ProjectileGone(0)));
    assert!(ev.contains(&CombatEvent::ProjectileGone(1)));
    assert_eq!(c.live_bombs(), 0);
    let log = p.tick(&mut c, &map, &d);
    assert_eq!(explosions(&log), at, "one per bomb, where it lay");
    // A rocket in flight is not a remote bomb.
    p.launch(&mut c, &map, &d, WeaponId::Rockets, Vec3::X);
    assert_eq!(c.live_bombs(), 0);
    let ev = c.player_attack(&map, &d, p.eye(), p.body.sector, &WeaponEvent::Detonate);
    assert!(ev.is_empty(), "{ev:?}");
    assert_eq!(c.projectiles.len(), 1);
}

#[test]
fn thrown_last_bomb_keeps_launcher_while_live() {
    let d = defs();
    let map = combat_room();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let mut a = Arsenal::new(&d);
    a.owned[WeaponId::PipeBombs.index()] = true;
    a.current = WeaponId::PipeBombs;
    a.reserve[AmmoKind::Bombs.index()] = 1;
    let mut rng = crate::rng::Rng::new(1);
    let press = WeaponInput {
        fire: true,
        fire_pressed: true,
        ..Default::default()
    };
    let idle = WeaponInput::default();
    // The game's order: live count, arsenal, attacks, combat tick.
    let mut step = |input: &WeaponInput, a: &mut Arsenal, c: &mut Combat, p: &mut Player| {
        a.live_bombs = c.live_bombs();
        let ev = a.tick(&d, input, Vec3::X, &mut rng, DT);
        let mut out = Vec::new();
        for e in &ev {
            out.extend(c.player_attack(&map, &d, p.eye(), p.body.sector, e));
        }
        out.extend(p.tick(c, &map, &d));
        (ev, out)
    };
    let (ev, _) = step(&press, &mut a, &mut c, &mut p);
    assert!(matches!(ev[..], [WeaponEvent::Launch { .. }]), "{ev:?}");
    for _ in 0..120 {
        step(&idle, &mut a, &mut c, &mut p);
        assert_eq!(a.current, WeaponId::PipeBombs);
        assert!(!matches!(a.phase, WeaponPhase::Switching { .. }));
    }
    let (ev, out) = step(&press, &mut a, &mut c, &mut p);
    assert_eq!(ev, vec![WeaponEvent::Detonate]);
    assert_eq!(explosions(&out).len(), 1);
    // Nothing live and nothing left: the next ticks put the launcher away.
    for _ in 0..60 {
        step(&idle, &mut a, &mut c, &mut p);
    }
    assert_ne!(a.current, WeaponId::PipeBombs);
}

/// A bomb resting on the floor of `map` at `(x, y)`.
fn resting_bomb(c: &mut Combat, map: &Map, d: &Defs, x: f32, y: f32) {
    let pd = proj_def(d, WeaponId::PipeBombs);
    let sector = map.find_sector(Vec2::new(x, y), None).unwrap();
    let pos = Vec3::new(x, y, map.sectors[sector].floor_z + pd.radius);
    c.projectiles.push(Projectile {
        id: 99,
        pos,
        prev: pos,
        vel: Vec3::ZERO,
        sector,
        radius: pd.radius,
        damage: pd.damage,
        owner: Shooter::Player,
        targets: Targets::All,
        life: pd.life,
        gravity: pd.gravity,
        bounce: pd.bounce,
        remote: pd.remote,
        splash: pd.splash,
        resting: true,
    });
}

#[test]
fn resting_bomb_follows_lift_floor() {
    let d = defs();
    let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    resting_bomb(&mut c, &map, &d, 5.0, 2.0);
    let r = c.projectiles[0].radius;
    mech.toggle(0);
    for _ in 0..180 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
        let b = &c.projectiles[0];
        assert!(b.resting);
        // The floor moved after this tick's combat step: at most one tick behind.
        assert!(
            (b.pos.z - (map.sectors[1].floor_z + r)).abs() < 0.1,
            "{}",
            b.pos.z
        );
    }
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
    p.tick(&mut c, &map, &d);
    assert!((c.projectiles[0].pos.z - (2.0 + r)).abs() < 1e-4);
    // And back down with it.
    mech.toggle(0);
    for _ in 0..180 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
        assert!(c.projectiles[0].resting);
    }
    p.tick(&mut c, &map, &d);
    assert!((c.projectiles[0].pos.z - r).abs() < 1e-4);
    // A floor that drops away at once leaves the bomb to fall.
    map.sectors[1].floor_z = -1.0;
    p.tick(&mut c, &map, &d);
    assert!(!c.projectiles[0].resting);
    for _ in 0..120 {
        p.tick(&mut c, &map, &d);
    }
    let b = &c.projectiles[0];
    assert!(b.resting, "settled again");
    assert!((b.pos.z - (-1.0 + r)).abs() < 0.05, "{}", b.pos.z);
}

/// Three barrels 3 m apart along y = 6.5: each blast reaches only the next one.
fn barrel_row(map: &mut Map) {
    for x in [1.0, 4.0, 7.0] {
        spawn_kind(map, ActorKind::Barrel, x, 6.5, 0.0, true);
    }
}

#[test]
fn barrel_chain_explodes_each_once() {
    let d = defs();
    let mut map = combat_room();
    barrel_row(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    // Bullets set the first one off.
    let aim = p.aim_at(&c, 0);
    let mut log: Vec<(usize, CombatEvent)> = Vec::new();
    for _ in 0..2 {
        let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![aim]);
        log.extend(ev.into_iter().map(|e| (0, e)));
    }
    assert!(log.iter().any(|(_, e)| *e == CombatEvent::ActorKilled(0)));
    for t in 1..=120 {
        log.extend(p.tick(&mut c, &map, &d).into_iter().map(|e| (t, e)));
    }
    for i in 0..3 {
        assert_eq!(
            log.iter()
                .filter(|(_, e)| *e == CombatEvent::ActorKilled(i))
                .count(),
            1,
            "barrel {i}"
        );
    }
    let booms: Vec<(usize, Vec3)> = log
        .iter()
        .filter_map(|(t, e)| match e {
            CombatEvent::Explosion { point, radius } => {
                assert_eq!(*radius, 4.0);
                Some((*t, *point))
            }
            _ => None,
        })
        .collect();
    assert_eq!(booms.len(), 3, "{booms:?}");
    for (i, (t, at)) in booms.iter().enumerate() {
        let b = c.actors[i].body.pos;
        assert!(at.truncate().distance(b.truncate()) < 1e-4, "barrel {i}");
        if i > 0 {
            // The fuse (0.15 s) spreads the chain over separate ticks.
            assert!(t - booms[i - 1].0 >= 8, "{booms:?}");
        }
    }
    assert!(c.pending_blasts.is_empty());
    assert_eq!(p.vitals.health.hp, PLAYER_MAX_HEALTH, "out of reach");
}

#[test]
fn two_splashes_one_barrel_one_explosion() {
    let d = defs();
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Barrel, 6.0, 6.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    for y in [5.5, 7.5] {
        c.pending_blasts.push(PendingBlast {
            fuse: 0.0,
            blast: Blast {
                center: Vec3::new(6.0, y, 0.5),
                sector: 0,
                splash: rocket_splash(&d),
                owner: Shooter::Player,
            },
            exclude: None,
        });
    }
    let ev = p.tick(&mut c, &map, &d);
    assert_eq!(explosions(&ev).len(), 2);
    assert_eq!(count(&ev, |e| *e == CombatEvent::ActorKilled(0)), 1);
    assert_eq!(c.pending_blasts.len(), 1, "the barrel's own, on its fuse");
    let mut log = ev;
    for _ in 0..60 {
        log.extend(p.tick(&mut c, &map, &d));
    }
    assert_eq!(explosions(&log).len(), 3);
    assert_eq!(count(&log, |e| *e == CombatEvent::ActorKilled(0)), 1);
}

#[test]
fn chain_terminates() {
    let d = defs();
    let mut map = combat_room();
    // A packed 4 x 2 stack: every blast reaches several others.
    for x in [1.0, 2.0, 3.0, 4.0] {
        for y in [6.0, 7.0] {
            spawn_kind(&mut map, ActorKind::Barrel, x, y, 0.0, true);
        }
    }
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 7.0, 1.5);
    p.vitals.health.hp = 10_000;
    c.pending_blasts.push(PendingBlast {
        fuse: 0.0,
        blast: Blast {
            center: Vec3::new(0.5, 6.5, 0.5),
            sector: 0,
            splash: rocket_splash(&d),
            owner: Shooter::Player,
        },
        exclude: None,
    });
    let mut log = Vec::new();
    for _ in 0..600 {
        log.extend(p.tick(&mut c, &map, &d));
    }
    assert!(c.pending_blasts.is_empty());
    assert!(c.actors.iter().all(|a| !a.alive()));
    for i in 0..c.actors.len() {
        assert_eq!(count(&log, |e| *e == CombatEvent::ActorKilled(i)), 1);
    }
    assert_eq!(explosions(&log).len(), 1 + c.actors.len());
    for _ in 0..60 {
        assert!(p.tick(&mut c, &map, &d).is_empty(), "all quiet");
    }
}

#[test]
fn grunt_killed_by_splash_reports_once() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 6.5, PI, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    c.pending_blasts.push(PendingBlast {
        fuse: 0.0,
        blast: Blast {
            center: Vec3::new(6.5, 6.5, 0.5),
            sector: 0,
            splash: rocket_splash(&d),
            owner: Shooter::Player,
        },
        exclude: None,
    });
    let ev = p.tick(&mut c, &map, &d);
    assert_eq!(
        count(&ev, |e| *e == CombatEvent::ActorKilled(0)),
        1,
        "{ev:?}"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, CombatEvent::ActorHurt { actor: 0, .. }))
    );
    assert!(
        c.pending_blasts.is_empty(),
        "a grunt leaves no blast behind"
    );
}
