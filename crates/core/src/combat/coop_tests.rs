//! Several players in one combat tick: whom actors hunt, who gets hurt, separation and the
//! per-player clocks.

use super::tests::{DT, Player, count, spawn_at, spawn_kind};
use super::*;
use crate::actors::{GRUDGE_SECS, SWITCH_RATIO, choose_player};
use crate::defs::WeaponId;
use crate::fixtures::{defs, engine_room, pillar_room, two_rooms};
use crate::hazard::HazardKind;
use crate::map::ActorKind;
use crate::players::PlayerId;
use crate::weapons::WeaponEvent;

/// One combat tick for every player in `ps` (`ps[k]` is `PlayerId(k)`), with a throwaway copy
/// of the mechanics.
fn tick_all(c: &mut Combat, map: &mut Map, d: &Defs, ps: &mut [Player]) -> Vec<CombatEvent> {
    let mut mech = Mechanics::new(&mut map.clone());
    let mut targets: Vec<PlayerTarget> = ps
        .iter_mut()
        .map(|p| {
            let eye = p.eye();
            PlayerTarget {
                body: &mut p.body,
                vitals: &mut p.vitals,
                eye,
            }
        })
        .collect();
    c.tick(map, &mut mech, d, &mut targets, DT)
}

fn hurt_ids(ev: &[CombatEvent]) -> Vec<PlayerId> {
    ev.iter()
        .filter_map(|e| match e {
            CombatEvent::PlayerHurt { player, .. } => Some(*player),
            _ => None,
        })
        .collect()
}

fn launch(
    c: &mut Combat,
    map: &mut Map,
    d: &Defs,
    by: PlayerId,
    p: &Player,
    w: WeaponId,
    dir: Vec3,
) {
    let ev = WeaponEvent::Launch { weapon: w, dir };
    c.player_attack(map, d, by, p.eye(), p.body.sector, &ev);
}

#[test]
fn actor_targets_nearest_visible_player() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 0.5, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 9.5, 8.0), Player::at(&map, 4.5, 8.0)];
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
}

#[test]
fn hidden_near_player_is_not_chosen() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 2.5, 5.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    // Player 1 is nearer, but the pillar (x,y∈[4,6]) is in the way.
    let mut ps = [Player::at(&map, 2.5, 9.5), Player::at(&map, 6.5, 5.0)];
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(0)));
}

#[test]
fn target_does_not_flip_between_equal_players() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    spawn_at(&mut map, 3.9, 3.5, -std::f32::consts::FRAC_PI_2, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 0.9, 0.5), Player::at(&map, 6.9, 0.5)];
    let first = {
        tick_all(&mut c, &mut map, &d, &mut ps);
        c.actors[0].target
    };
    for _ in 1..60 {
        tick_all(&mut c, &mut map, &d, &mut ps);
        assert_eq!(c.actors[0].target, first);
    }
}

#[test]
fn target_of_missing_player_repicks() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 0.5, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].target = Target::Player(PlayerId(3));
    let mut ps = [Player::at(&map, 9.5, 8.0), Player::at(&map, 4.5, 8.0)];
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert!(
        matches!(c.actors[0].target, Target::Player(PlayerId(k)) if k < 2),
        "{:?}",
        c.actors[0].target
    );
}

#[test]
fn all_players_dead_actor_keeps_valid_target() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 0.5, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 9.5, 8.0), Player::at(&map, 4.5, 8.0)];
    for p in &mut ps {
        p.vitals.damage(10_000);
        assert!(!p.vitals.health.alive());
    }
    for _ in 0..120 {
        let ev = tick_all(&mut c, &mut map, &d, &mut ps);
        assert_eq!(
            count(&ev, |e| matches!(e, CombatEvent::ActorFired { .. })),
            0
        );
        assert!(matches!(c.actors[0].target, Target::Player(PlayerId(k)) if k < 2));
    }
}

#[test]
fn hurt_by_other_player_turns_on_them() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 5.0, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(0)));
    let p1 = Player::at(&map, 1.0, 8.0);
    let ev = WeaponEvent::Fire {
        weapon: WeaponId::Pistol,
        dirs: vec![p1.aim_at(&c, 0)],
    };
    let out = c.player_attack(&mut map, &d, PlayerId(1), p1.eye(), p1.body.sector, &ev);
    assert!(out.contains(&CombatEvent::ActorHurt {
        actor: 0,
        amount: 12
    }));
    assert!(c.actors[0].alive());
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
}

/// The shooter stays the target for `GRUDGE_SECS` even with a nearer visible player, then the
/// usual choice comes back.
#[test]
fn grudge_keeps_attacker_then_expires() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 2.0, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 4.0, 8.0), Player::at(&map, 9.5, 8.0)];
    let ev = WeaponEvent::Fire {
        weapon: WeaponId::Pistol,
        dirs: vec![ps[1].aim_at(&c, 0)],
    };
    let (eye, sector) = (ps[1].eye(), ps[1].body.sector);
    c.player_attack(&mut map, &d, PlayerId(1), eye, sector, &ev);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
    let spot = c.actors[0].body;
    let mut tick = |c: &mut Combat, ps: &mut [Player; 2]| {
        tick_all(c, &mut map, &d, ps);
        // Nobody dies or moves, so only the grudge decides.
        c.actors[0].body = spot;
        for p in ps.iter_mut() {
            p.vitals = Player::at(&map, 0.0, 0.0).vitals;
        }
    };
    let a = spot.pos + Vec3::Z * 0.9 * spot.height;
    let (d0, d1) = (ps[0].eye().distance(a), ps[1].eye().distance(a));
    assert!(d0 < SWITCH_RATIO * d1, "{d0} {d1}");
    let held = (GRUDGE_SECS / DT) as usize - 5;
    for k in 0..held {
        tick(&mut c, &mut ps);
        assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)), "tick {k}");
    }
    for _ in 0..10 {
        tick(&mut c, &mut ps);
    }
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(0)));
}

/// A sleeper only wakes to what it sees ahead: the player behind it does not hold its target.
#[test]
fn sleeper_turns_to_player_in_front() {
    let d = defs();
    let mut map = pillar_room();
    spawn_at(&mut map, 5.0, 8.0, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    // Player 0 is nearer and in plain line of sight, but behind (west of) the sleeper.
    let mut ps = [Player::at(&map, 3.5, 8.0), Player::at(&map, 9.5, 8.0)];
    let a = &c.actors[0];
    assert!(crate::trace::can_see(
        &map,
        a.eye(),
        a.body.sector,
        ps[0].eye()
    ));
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
    assert!(
        matches!(c.actors[0].state, AiState::Alert { .. }),
        "{:?}",
        c.actors[0].state
    );
}

#[test]
fn ai_runs_once_per_tick() {
    let d = defs();
    let mut map = pillar_room();
    // One asleep facing away, one awake and fighting player 0.
    spawn_at(&mut map, 1.0, 1.0, std::f32::consts::PI, true);
    spawn_at(&mut map, 1.0, 8.0, 0.0, false);
    let run = |players: usize| {
        let mut map = map.clone();
        let mut c = Combat::spawn(&map, &d, 7);
        let mut ps = vec![Player::at(&map, 3.0, 8.0)];
        if players == 2 {
            // Far away, behind the sleeper: nobody turns to it.
            ps.push(Player::at(&map, 9.5, 9.5));
        }
        for _ in 0..30 {
            tick_all(&mut c, &mut map, &d, &mut ps);
        }
        (c.rng, c.actors)
    };
    let (rng1, actors1) = run(1);
    let (rng2, actors2) = run(2);
    assert_eq!(rng1, rng2);
    assert_eq!(actors1, actors2);
}

#[test]
fn rocket_spares_other_player() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 1.5, 1.5), Player::at(&map, 1.5, 3.4)];
    // Straight down at the floor between them.
    let dir = (Vec3::new(1.5, 2.5, 0.0) - ps[0].eye()).normalize();
    launch(
        &mut c,
        &mut map,
        &d,
        PlayerId(0),
        &ps[0],
        WeaponId::Rockets,
        dir,
    );
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(tick_all(&mut c, &mut map, &d, &mut ps));
    }
    assert_eq!(
        count(&log, |e| matches!(e, CombatEvent::Explosion { .. })),
        1
    );
    let ids = hurt_ids(&log);
    assert!(ids.contains(&PlayerId(0)), "self splash: {log:?}");
    assert!(!ids.contains(&PlayerId(1)), "{log:?}");
}

#[test]
fn enemy_bolt_hits_either_player() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    spawn_at(&mut map, 0.5, 0.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 7.0, 2.0), Player::at(&map, 4.5, 2.0)];
    let from = Vec3::new(1.0, 2.0, 1.0);
    c.projectiles.push(Projectile {
        id: 99,
        pos: from,
        prev: from,
        // Aimed at player 0's chest.
        vel: Vec3::X * 20.0,
        sector: 0,
        radius: 0.1,
        damage: 10,
        owner: Shooter::Actor(0),
        targets: Targets::All,
        life: 5.0,
        gravity: 0.0,
        bounce: None,
        remote: false,
        splash: None,
        resting: false,
    });
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(tick_all(&mut c, &mut map, &d, &mut ps));
    }
    assert_eq!(hurt_ids(&log), vec![PlayerId(1)], "{log:?}");
}

#[test]
fn players_are_separated_from_each_other() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 2.0, 2.0), Player::at(&map, 2.0, 2.0)];
    tick_all(&mut c, &mut map, &d, &mut ps);
    let (a, b) = (ps[0].body, ps[1].body);
    let dist = a.pos.truncate().distance(b.pos.truncate());
    assert!(dist >= a.radius + b.radius - 1e-3, "{dist}");
}

#[test]
fn hazard_burns_each_player_separately() {
    let d = defs();
    let mut map = engine_room("");
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 2.0, 2.0), Player::at(&map, 11.0, 2.0)];
    let ev = tick_all(&mut c, &mut map, &d, &mut ps);
    let burns: Vec<&CombatEvent> = ev
        .iter()
        .filter(|e| matches!(e, CombatEvent::HazardBurn { .. }))
        .collect();
    assert_eq!(
        burns,
        vec![&CombatEvent::HazardBurn {
            player: PlayerId(1),
            kind: HazardKind::Slime
        }]
    );
    assert_eq!(hurt_ids(&ev), vec![PlayerId(1)]);
    assert_eq!(c.hazard.len(), 2);
}

#[test]
fn detonate_only_sets_off_own_bombs() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    let mut c = Combat::spawn(&map, &d, 1);
    let ps = [Player::at(&map, 1.0, 1.0), Player::at(&map, 1.0, 3.0)];
    launch(
        &mut c,
        &mut map,
        &d,
        PlayerId(0),
        &ps[0],
        WeaponId::PipeBombs,
        Vec3::X,
    );
    launch(
        &mut c,
        &mut map,
        &d,
        PlayerId(1),
        &ps[1],
        WeaponId::PipeBombs,
        Vec3::X,
    );
    assert_eq!(c.live_bombs(PlayerId(0)), 1);
    assert_eq!(c.live_bombs(PlayerId(1)), 1);
    let ev = c.player_attack(
        &mut map,
        &d,
        PlayerId(1),
        ps[1].eye(),
        ps[1].body.sector,
        &WeaponEvent::Detonate,
    );
    assert_eq!(count(&ev, |e| *e == CombatEvent::BombsDetonated), 1);
    assert_eq!(c.live_bombs(PlayerId(0)), 1);
    assert_eq!(c.live_bombs(PlayerId(1)), 0);
}

#[test]
fn choose_player_rules() {
    let map = pillar_room();
    let d = defs();
    let mut m = map.clone();
    spawn_at(&mut m, 0.5, 8.0, 0.0, false);
    let c = Combat::spawn(&m, &d, 1);
    let a = &c.actors[0];
    let g = d.enemy(a.kind);
    let view = |x: f32, y: f32, alive: bool| {
        let b = Body::spawn(&map, Vec2::new(x, y), 0.35, 1.8).unwrap();
        Perception {
            eye: b.pos + Vec3::Z * 1.6,
            chest: b.pos + Vec3::Z * 1.08,
            sector: b.sector,
            alive,
        }
    };
    // One view: always player 0.
    assert_eq!(
        choose_player(&map, a, g, PlayerId(2), &[view(9.0, 8.0, false)]),
        PlayerId(0)
    );
    // Current visible: kept unless another is nearer than SWITCH_RATIO of it.
    let near = 8.5 * SWITCH_RATIO + 0.5 + 0.2; // just outside the ratio
    let views = [view(9.0, 8.0, true), view(near, 8.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(0), &views), PlayerId(0));
    let views = [view(9.0, 8.0, true), view(3.0, 8.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(0), &views), PlayerId(1));
    // Beyond sight range counts as unseen: the current one at 8.5 m is kept against one at
    // 6.5 m (not within the ratio) until the range shrinks below 8.5 m.
    let views = [view(7.0, 8.0, true), view(9.0, 8.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(1), &views), PlayerId(1));
    let mut short = g.clone();
    short.sight_range = 7.0;
    assert_eq!(
        choose_player(&map, a, &short, PlayerId(1), &views),
        PlayerId(0)
    );
    // Current dead: nearest visible living one.
    let views = [view(2.0, 8.0, false), view(9.0, 8.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(0), &views), PlayerId(1));
    // None visible (both behind the pillar from (0.5, 8)): keep current while alive...
    let views = [view(9.5, 1.0, true), view(7.0, 3.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(0), &views), PlayerId(0));
    // ...else the nearest living one; none living: player 0.
    let views = [view(9.5, 1.0, false), view(7.0, 3.0, true)];
    assert_eq!(choose_player(&map, a, g, PlayerId(0), &views), PlayerId(1));
    let views = [view(9.5, 1.0, false), view(7.0, 3.0, false)];
    assert_eq!(choose_player(&map, a, g, PlayerId(1), &views), PlayerId(0));
}

/// An enemy bolt flies through a dead player to the living one behind.
#[test]
fn dead_partner_does_not_block_bolts() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    spawn_at(&mut map, 0.5, 0.5, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 4.5, 2.0), Player::at(&map, 7.0, 2.0)];
    ps[0].vitals.damage(10_000);
    let from = Vec3::new(1.0, 2.0, 1.0);
    c.projectiles.push(Projectile {
        id: 99,
        pos: from,
        prev: from,
        // Through player 0's chest at player 1's.
        vel: Vec3::X * 20.0,
        sector: 0,
        radius: 0.1,
        damage: 10,
        owner: Shooter::Actor(0),
        targets: Targets::All,
        life: 5.0,
        gravity: 0.0,
        bounce: None,
        remote: false,
        splash: None,
        resting: false,
    });
    let mut log = Vec::new();
    for _ in 0..30 {
        log.extend(tick_all(&mut c, &mut map, &d, &mut ps));
    }
    assert_eq!(hurt_ids(&log), vec![PlayerId(1)], "{log:?}");
}

/// A hitscan volley passes a dead player and hits the living one behind.
#[test]
fn dead_partner_does_not_block_pellets() {
    let mut d = defs();
    for e in &mut d.enemies {
        e.aim_time = 0.0;
        e.strafe = false;
    }
    let mut map = two_rooms(0.0, 3.0);
    spawn_kind(&mut map, ActorKind::Enforcer, 1.0, 2.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    // Player 0 stands dead right in front of the muzzle.
    let mut ps = [Player::at(&map, 2.0, 2.0), Player::at(&map, 6.0, 2.0)];
    ps[0].vitals.damage(10_000);
    let mut log = Vec::new();
    for _ in 0..120 {
        log.extend(tick_all(&mut c, &mut map, &d, &mut ps));
        if count(&log, |e| matches!(e, CombatEvent::ActorFired { .. })) > 0 {
            break;
        }
    }
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
    assert!(
        count(&log, |e| matches!(e, CombatEvent::ActorFired { .. })) > 0,
        "{log:?}"
    );
    assert!(hurt_ids(&log).contains(&PlayerId(1)), "{log:?}");
}

/// A dead player is neither pushed by an actor nor by its partner; those two still separate.
#[test]
fn dead_partner_is_not_pushed() {
    let d = defs();
    let mut map = two_rooms(0.0, 3.0);
    // Asleep and facing away (west) from both players.
    spawn_at(&mut map, 2.5, 2.0, std::f32::consts::PI, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut ps = [Player::at(&map, 2.0, 2.0), Player::at(&map, 2.25, 2.1)];
    ps[1].vitals.damage(10_000);
    let dead = ps[1].body.pos;
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert_eq!(ps[1].body.pos, dead);
    let (p, a) = (ps[0].body, c.actors[0].body);
    let dist = p.pos.truncate().distance(a.pos.truncate());
    assert!(dist >= p.radius + a.radius - 1e-3, "{dist}");
}

/// A static actor (a barrel) keeps the player it has, even a dead one: it never hunts, so it
/// never re-picks.
#[test]
fn static_actor_keeps_its_target() {
    let d = defs();
    let mut map = pillar_room();
    spawn_kind(&mut map, ActorKind::Barrel, 1.0, 8.0, 0.0, false);
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].target = Target::Player(PlayerId(1));
    // Player 0 is far nearer, in plain sight and alive; player 1 is dead.
    let mut ps = [Player::at(&map, 2.0, 8.0), Player::at(&map, 9.5, 8.0)];
    ps[1].vitals.damage(10_000);
    tick_all(&mut c, &mut map, &d, &mut ps);
    assert_eq!(c.actors[0].target, Target::Player(PlayerId(1)));
}
