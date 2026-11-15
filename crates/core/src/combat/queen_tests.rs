//! The Hive Queen: acid volleys, phases at 60 % and 30 %, the burrow cycle between burrow
//! points (untargetable underground, minions on the way up, capped per skill), the enraged
//! ground pound, and no infighting.

use super::specials::pound_damage;
use super::tests::{DT, Player, count, spawn_kind};
use super::*;
use crate::actors::Target;
use crate::actors::special::{PLAYER_CLEAR, pick_point};
use crate::defs::WeaponId;
use crate::defs::{BurrowDef, PoundDef};
use crate::difficulty::Difficulty;
use crate::fixtures::defs;
use crate::map::{ActorKind, ActorSpawn};
use crate::weapons::WeaponEvent;
use std::f32::consts::PI;

/// A 30×30 m hall (floor 0, ceiling 6) with the Queen (actor 0) at (15, 15) facing west and
/// the given burrow points. Start (2, 2).
fn hive(points: &str) -> Map {
    let mut map = Map::from_ron(&format!(
        r#"(
        name: "hive", materials: ["wall", "floor", "ceiling"],
        vertices: [(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)],
        sectors: [(loops: [[0, 1, 2, 3]], floor_z: 0.0, ceil_z: 6.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0)],
        player_start: (pos: (2.0, 2.0), angle_deg: 0.0),
        burrow_points: [{points}],
    )"#
    ))
    .unwrap();
    map.actors.push(ActorSpawn {
        kind: ActorKind::HiveQueen,
        pos: Vec2::new(15.0, 15.0),
        angle: PI,
        asleep: false,
        skill: Difficulty::Easy,
        on_death: None,
    });
    map
}

const FOUR: &str = "(5.0, 25.0), (25.0, 25.0), (25.0, 5.0), (15.0, 15.0)";

/// A player that can take a beating.
fn tough(map: &Map, x: f32, y: f32) -> Player {
    let mut p = Player::at(map, x, y);
    p.vitals.health.max = 1_000_000;
    p.vitals.health.hp = 1_000_000;
    p
}

/// The Queen awake and chasing in `phase` (health to match), its moves due at once.
fn in_phase(c: &mut Combat, d: &Defs, phase: u8) {
    let q = &mut c.actors[0];
    q.health.hp = q.health.max * [10, 5, 2][phase as usize] / 10;
    q.phase = phase;
    q.state = AiState::Chase;
    let def = d.enemy(ActorKind::HiveQueen).phased(phase);
    q.muzzle_offset = Vec3::from(def.muzzle);
    q.burrow_in = 0.0;
    q.pound_in = 0.0;
}

fn burrow_plan(d: &Defs) -> BurrowDef {
    d.enemy(ActorKind::HiveQueen).phased(1).burrow.unwrap()
}

fn pound_plan(d: &Defs) -> PoundDef {
    d.enemy(ActorKind::HiveQueen).phased(2).pound.unwrap()
}

#[test]
fn queen_def_spits_then_burrows_then_pounds() {
    let d = defs();
    let q = d.enemy(ActorKind::HiveQueen);
    assert!(q.boss && q.burrows());
    assert!(matches!(q.attack, EnemyAttack::Bolts { proj, burst: 3, .. } if proj.splash.is_some()));
    assert_eq!(q.muzzle, (1.68, 0.0, 1.47), "the Mouth bone at the spit");
    assert_eq!((q.radius, q.height), (1.4, 3.0));
    let phases = [1.0, 0.61, 0.59, 0.31, 0.29, 0.01].map(|f| q.phase_for(f));
    assert_eq!(phases, [0, 0, 1, 1, 2, 2]);
    assert!(
        q.burrow.is_none() && q.pound.is_none(),
        "phase 1 only spits"
    );
    let p1 = q.phased(1);
    assert!(p1.burrow.is_some() && p1.pound.is_none());
    let b = p1.burrow.unwrap();
    assert_eq!((b.every, b.spawn), ((8.0, 10.0), 2));
    let p2 = q.phased(2);
    assert!(p2.burrow.is_none() && p2.pound.is_some());
    assert!(
        p2.attack_refire < p1.attack_refire,
        "enraged: faster volleys"
    );
    assert_eq!(p2.pound.unwrap().every, 6.0);
    let m = q.minions.unwrap();
    assert_eq!(m.kind, ActorKind::Slasher, "the Gnasher model");
    let caps = Difficulty::ALL.map(|s| m.cap.get(s));
    assert_eq!(caps, [4, 6, 8]);
}

#[test]
fn damage_changes_phase_at_sixty_and_thirty_percent() {
    let map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let max = c.actors[0].health.max;
    let phase_of = |ev: &[CombatEvent]| {
        ev.iter().find_map(|e| match e {
            CombatEvent::PhaseChanged { phase, .. } => Some(*phase),
            _ => None,
        })
    };
    // Down to just above 60 %: still phase 0.
    let ev = c.damage_actor(&d, 0, max * 4 / 10 - 1, Shooter::Player);
    assert_eq!(phase_of(&ev), None);
    let ev = c.damage_actor(&d, 0, 2, Shooter::Player);
    assert_eq!(phase_of(&ev), Some(1));
    let b = burrow_plan(&d);
    assert_eq!(
        c.actors[0].burrow_in, b.first,
        "the first burrow comes soon"
    );
    let to_thirty = c.actors[0].health.hp - max * 3 / 10 + 1;
    let ev = c.damage_actor(&d, 0, to_thirty, Shooter::Player);
    assert_eq!(phase_of(&ev), Some(2));
    assert_eq!(c.actors[0].pound_in, pound_plan(&d).first);
}

/// Ticks until `f` holds, returning the tick count; panics after `max` ticks.
fn until(
    c: &mut Combat,
    map: &mut Map,
    d: &Defs,
    p: &mut Player,
    max: usize,
    mut f: impl FnMut(&Combat, &[CombatEvent]) -> bool,
) -> usize {
    for t in 1..=max {
        let ev = p.tick(c, map, d);
        if f(c, &ev) {
            return t;
        }
    }
    panic!("condition not met in {max} ticks");
}

#[test]
fn the_burrow_cycle_runs_dig_under_emerge_then_waits() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 7);
    in_phase(&mut c, &d, 1);
    let mut p = tough(&map, 3.0, 3.0);
    let b = burrow_plan(&d);
    let secs = |t: usize| t as f32 * DT;
    let burrowed =
        |_: &Combat, ev: &[CombatEvent]| ev.contains(&CombatEvent::Burrowed { actor: 0 });
    until(&mut c, &mut map, &d, &mut p, 2, burrowed);
    assert!(matches!(c.actors[0].state, AiState::Burrow { .. }));
    assert!(c.actors[0].targetable(), "still a target while digging");
    let dig = until(&mut c, &mut map, &d, &mut p, 600, |c, _| {
        matches!(c.actors[0].state, AiState::Under { .. })
    });
    assert!((secs(dig) - b.dig).abs() <= 2.0 * DT, "dig {}", secs(dig));
    assert!(!c.actors[0].targetable() && c.actors[0].alive());
    let under = until(&mut c, &mut map, &d, &mut p, 600, |_, ev| {
        ev.contains(&CombatEvent::Surfaced { actor: 0 })
    });
    assert!(
        (secs(under) - b.under).abs() <= 2.0 * DT,
        "under {}",
        secs(under)
    );
    assert!(matches!(c.actors[0].state, AiState::Emerge { .. }));
    let rise = until(&mut c, &mut map, &d, &mut p, 600, |c, _| {
        !matches!(c.actors[0].state, AiState::Emerge { .. })
    });
    assert!(
        (secs(rise) - b.rise).abs() <= 2.0 * DT,
        "rise {}",
        secs(rise)
    );
    // The next burrow comes 8-10 s of chasing later (a burst in progress may delay it a little).
    let wait = until(&mut c, &mut map, &d, &mut p, 1200, burrowed);
    let (lo, hi) = b.every;
    assert!(
        secs(wait) >= lo - DT && secs(wait) <= hi + 1.0,
        "next burrow after {} s",
        secs(wait)
    );
}

#[test]
fn underground_she_cannot_be_hit_or_touched() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 1);
    let plan = burrow_plan(&d);
    c.actors[0].state = AiState::Under { t: 1.0, plan };
    let hp = c.actors[0].health.hp;
    // A pistol shot straight at her passes and hits the far wall.
    let p = Player::at(&map, 5.0, 15.0);
    let aim = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &mut map, &d, WeaponId::Pistol, vec![aim]);
    assert_eq!(
        count(&ev, |e| matches!(e, CombatEvent::ActorHurt { .. })),
        0
    );
    assert_eq!(
        count(&ev, |e| matches!(e, CombatEvent::Impact { .. })),
        1,
        "{ev:?}"
    );
    // Direct damage and blasts are ignored too.
    assert!(c.damage_actor(&d, 0, 50, Shooter::Player).is_empty());
    assert_eq!(c.actors[0].health.hp, hp);
    // No collision: she is out of the movers' bodies and the player walks through her.
    assert!(c.living_bodies().0.is_empty());
    let mut p = tough(&map, 15.5, 15.0);
    let before = p.body.pos;
    p.tick(&mut c, &mut map, &d);
    assert_eq!(p.body.pos, before, "not pushed apart");
    // A rocket fired at her flies through.
    let ev = WeaponEvent::Launch {
        weapon: WeaponId::Rockets,
        dir: Vec3::X,
    };
    let eye = Vec3::new(5.0, 15.0, 1.6);
    c.player_attack(&mut map, &d, eye, 0, &ev);
    let mut p = tough(&map, 2.0, 2.0);
    for _ in 0..30 {
        p.tick(&mut c, &mut map, &d);
    }
    assert_eq!(c.actors[0].health.hp, hp, "the rocket passed");
}

#[test]
fn she_comes_up_at_another_point_away_from_the_player() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 3);
    in_phase(&mut c, &d, 1);
    // The player stands by point 0, so she never picks it while others are free.
    let mut p = tough(&map, 5.0, 23.0);
    let plan = burrow_plan(&d);
    let mut at = vec![];
    for _ in 0..8 {
        c.actors[0].state = AiState::Under { t: 0.0, plan };
        let ev = p.tick(&mut c, &mut map, &d);
        assert!(ev.contains(&CombatEvent::Surfaced { actor: 0 }));
        let k = c.actors[0].burrow_at.unwrap();
        assert_eq!(c.actors[0].body.pos.truncate(), map.burrow_points[k]);
        assert_eq!(
            c.actors[0].prev_pos, c.actors[0].body.pos,
            "no slide across the map"
        );
        at.push(k);
    }
    // She starts on point 3 (the centre), and never comes up where she went down.
    assert_ne!(at[0], 3);
    assert!(at.windows(2).all(|w| w[0] != w[1]), "{at:?}");
    assert!(!at.contains(&0), "never next to the player: {at:?}");
}

#[test]
fn the_point_pick_avoids_the_current_point_and_the_player() {
    let map = hive("(5.0, 5.0), (25.0, 5.0), (25.0, 25.0)");
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].burrow_at = Some(1);
    let near0 = map.burrow_points[0] + Vec2::new(1.0, 0.0);
    for seed in 0..20 {
        let mut rng = Rng::new(seed);
        assert_eq!(pick_point(&map, &c.actors[0], near0, &mut rng), Some(2));
    }
    // With the player on the only other point she still goes there.
    let two = hive("(5.0, 5.0), (25.0, 5.0)");
    let mut c2 = Combat::spawn(&two, &d, 1);
    c2.actors[0].burrow_at = Some(0);
    let on1 = two.burrow_points[1];
    assert!(on1.distance(on1) < PLAYER_CLEAR);
    let mut rng = Rng::new(9);
    assert_eq!(pick_point(&two, &c2.actors[0], on1, &mut rng), Some(1));
    // Fewer than two points: nowhere to go.
    let one = hive("(5.0, 5.0)");
    let c1 = Combat::spawn(&one, &d, 1);
    assert_eq!(pick_point(&one, &c1.actors[0], on1, &mut rng), None);
}

#[test]
fn with_fewer_than_two_points_she_never_burrows() {
    for points in ["", "(5.0, 25.0)"] {
        let mut map = hive(points);
        let d = defs();
        let mut c = Combat::spawn(&map, &d, 1);
        in_phase(&mut c, &d, 1);
        let mut p = tough(&map, 3.0, 3.0);
        for _ in 0..(15.0 / DT) as usize {
            let ev = p.tick(&mut c, &mut map, &d);
            assert!(!ev.iter().any(|e| matches!(e, CombatEvent::Burrowed { .. })));
        }
    }
}

#[test]
fn surfacing_brings_up_two_awake_gnashers() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 1);
    let mut p = tough(&map, 3.0, 3.0);
    c.actors[0].state = AiState::Under {
        t: 0.0,
        plan: burrow_plan(&d),
    };
    let ev = p.tick(&mut c, &mut map, &d);
    let spawned: Vec<usize> = ev
        .iter()
        .filter_map(|e| match e {
            CombatEvent::ActorSpawned(i) => Some(*i),
            _ => None,
        })
        .collect();
    assert_eq!(spawned, [1, 2]);
    let q = c.actors[0].body;
    for &i in &spawned {
        let m = &c.actors[i];
        assert_eq!(m.kind, ActorKind::Slasher);
        assert_eq!(m.summoner, Some(0));
        assert_ne!(m.state, AiState::Sleep, "awake");
        let gap = m.body.pos.truncate().distance(q.pos.truncate());
        assert!(gap < q.radius + m.body.radius + 1.0, "next to her: {gap}");
    }
    assert_eq!(c.live_minions(0), 2);
}

#[test]
fn live_minions_are_capped_per_skill() {
    let d = defs();
    for (skill, cap) in [
        (Difficulty::Easy, 4),
        (Difficulty::Normal, 6),
        (Difficulty::Hard, 8),
    ] {
        let map = hive(FOUR);
        let mut c = Combat::spawn(&map, &d, 1);
        c.skill = skill;
        let player = Player::at(&map, 2.0, 2.0).body;
        let mut out = Vec::new();
        // Surface at fresh spots of the hall so every minion fits.
        for k in 0..6 {
            let at = Vec2::new(6.0 + 6.0 * (k % 3) as f32, 8.0 + 10.0 * (k / 3) as f32);
            c.actors[0].body.pos = at.extend(0.0);
            c.spawn_minions(&map, &d, 0, 2, &player, &mut out);
        }
        assert_eq!(c.live_minions(0), cap, "{skill:?}");
        // A dead minion frees one place.
        let m = c.actors.iter().position(|a| a.summoner == Some(0)).unwrap();
        c.damage_actor(&d, m, 1000, Shooter::Player);
        c.actors[0].body.pos = Vec3::new(15.0, 25.0, 0.0);
        let before = c.actors.len();
        c.spawn_minions(&map, &d, 0, 2, &player, &mut out);
        assert_eq!(c.actors.len(), before + 1, "{skill:?}");
        assert_eq!(c.live_minions(0), cap);
    }
}

#[test]
fn ground_pound_falls_off_and_spares_the_airborne() {
    let map = hive(FOUR);
    let plan = pound_plan(&defs());
    let centre = Vec3::new(15.0, 15.0, 0.0);
    let at = |x: f32| Player::at(&map, x, 15.0).body;
    let hit = |b: &Body| pound_damage(&map, centre, 0, plan, b);
    let near = hit(&at(16.0)).unwrap();
    let mid = hit(&at(18.0)).unwrap();
    assert!(near > mid && mid > 0, "{near} > {mid}");
    assert_eq!(
        near,
        (plan.damage as f32 * (1.0 - 1.0 / plan.radius)).round() as i32
    );
    assert_eq!(hit(&at(15.0 + plan.radius + 0.1)), None, "out of reach");
    let mut jumping = at(16.0);
    jumping.on_ground = false;
    jumping.pos.z += 0.4;
    assert_eq!(hit(&jumping), None, "airborne");
    let mut high = at(16.0);
    high.pos.z += 2.0;
    assert_eq!(hit(&high), None, "on a ledge above");
}

#[test]
fn the_enraged_queen_pounds_bodies_on_the_ground() {
    let mut map = hive(FOUR);
    spawn_kind(&mut map, ActorKind::Grunt, 18.0, 15.0, 0.0, true); // actor 1, 3 m off
    spawn_kind(&mut map, ActorKind::Drone, 15.0, 18.0, 0.0, true); // actor 2, hovering
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 2);
    let mut p = tough(&map, 13.0, 15.0);
    let ev = p.tick(&mut c, &mut map, &d);
    assert!(matches!(c.actors[0].state, AiState::Pound { .. }), "{ev:?}");
    let plan = pound_plan(&d);
    let windup = until(&mut c, &mut map, &d, &mut p, 120, |_, ev| {
        ev.iter()
            .any(|e| matches!(e, CombatEvent::GroundPound { actor: 0, .. }))
    });
    assert!((windup as f32 * DT - plan.windup).abs() <= 2.0 * DT);
    let grunt = c.actors[1].health;
    assert!(grunt.hp < grunt.max, "the grunt on the ground was hit");
    assert_eq!(
        c.actors[2].health.hp, c.actors[2].health.max,
        "the drone was not"
    );
    assert!(
        p.vitals.health.hp < p.vitals.health.max,
        "the player was hit"
    );
    // The next one comes no sooner than `every` seconds later.
    let next = until(&mut c, &mut map, &d, &mut p, 1200, |_, ev| {
        ev.iter()
            .any(|e| matches!(e, CombatEvent::GroundPound { .. }))
    });
    assert!(
        next as f32 * DT >= plan.every,
        "after {} s",
        next as f32 * DT
    );
}

#[test]
fn the_pound_spares_a_jumping_player() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 2);
    c.actors[0].state = AiState::Pound {
        t: 0.0,
        plan: pound_plan(&d),
    };
    let mut p = tough(&map, 13.0, 15.0);
    p.body.on_ground = false;
    let ev = p.tick(&mut c, &mut map, &d);
    assert!(
        ev.iter()
            .any(|e| matches!(e, CombatEvent::GroundPound { .. }))
    );
    assert!(
        !ev.iter()
            .any(|e| matches!(e, CombatEvent::PlayerHurt { .. })),
        "{ev:?}"
    );
    // On the ground the same slam hurts, scaled by the difficulty.
    c.damage_scale = 0.5;
    c.actors[0].state = AiState::Pound {
        t: 0.0,
        plan: pound_plan(&d),
    };
    p.body.on_ground = true;
    let ev = p.tick(&mut c, &mut map, &d);
    let full = pound_damage(&map, c.actors[0].body.pos, 0, pound_plan(&d), &p.body).unwrap();
    assert!(
        ev.contains(&CombatEvent::PlayerHurt {
            amount: ((full as f32) * 0.5).round() as i32,
            from: c.actors[0].body.pos,
        }),
        "{ev:?}"
    );
}

#[test]
fn she_never_infights_and_pain_never_breaks_a_burrow() {
    let mut map = hive(FOUR);
    spawn_kind(&mut map, ActorKind::Slasher, 18.0, 15.0, 0.0, false); // actor 1
    let mut d = defs();
    for e in &mut d.enemies {
        e.pain_chance = 1.0;
    }
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 1);
    c.damage_actor(&d, 0, 5, Shooter::Actor(1));
    assert_eq!(
        c.actors[0].target,
        Target::Player,
        "a boss only hunts the player"
    );
    let plan = burrow_plan(&d);
    c.actors[0].state = AiState::Burrow { t: 0.5, plan };
    c.actors[0].pain_ready_in = 0.0;
    c.damage_actor(&d, 0, 5, Shooter::Player);
    assert!(
        matches!(c.actors[0].state, AiState::Burrow { .. }),
        "no flinch mid-move"
    );
    // A kill still drops her, whatever the move.
    let hp = c.actors[0].health.hp;
    c.damage_actor(&d, 0, hp, Shooter::Player);
    assert!(matches!(c.actors[0].state, AiState::Dying { .. }));
}

#[test]
fn her_spit_comes_from_the_mouth_and_splashes() {
    let mut map = hive(FOUR);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    in_phase(&mut c, &d, 0);
    let mut p = tough(&map, 5.0, 15.0);
    until(&mut c, &mut map, &d, &mut p, 600, |c, _| {
        !c.projectiles.is_empty()
    });
    let bolt = &c.projectiles[0];
    assert!(bolt.splash.is_some());
    assert_eq!(bolt.owner, Shooter::Actor(0));
    let q = &c.actors[0];
    let mouth = q.muzzle();
    assert!(
        bolt.prev.distance(mouth) < 0.5,
        "{:?} vs {mouth:?}",
        bolt.prev
    );
    // Three spits per volley.
    let mut fired = 1;
    for _ in 0..60 {
        let ev = p.tick(&mut c, &mut map, &d);
        fired += count(&ev, |e| *e == CombatEvent::ActorFired { actor: 0 });
    }
    assert_eq!(fired, 3);
}
