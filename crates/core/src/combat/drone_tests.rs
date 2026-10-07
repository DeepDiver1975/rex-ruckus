//! The Drone: a flyer that hovers, swoops to shoot, ignores floor steps and lifts, and falls
//! when it dies.

use super::tests::{DT, Player, mech_tick, spawn_kind};
use super::*;
use crate::actors::next_hop;
use crate::fixtures::{defs, lift_shaft, two_rooms};
use crate::map::ActorKind;
use crate::movement::{Pass, step_flyer};

fn drone_def() -> crate::defs::EnemyDef {
    defs().enemy(ActorKind::Drone).clone()
}

#[test]
fn drone_def_matches_the_brief() {
    let d = drone_def();
    assert_eq!((d.health, d.speed), (25, 6.0));
    assert_eq!(
        d.locomotion,
        Locomotion::Fly {
            hover: 2.5,
            swoop: true
        }
    );
    assert_eq!(d.attack_refire, 1.1);
    let EnemyAttack::Bolts { proj, burst, .. } = d.attack else {
        panic!("a Drone fires bolts");
    };
    assert_eq!((proj.speed, proj.damage, burst), (18.0, 6, 1));
}

#[test]
fn drone_paths_over_step() {
    let d = drone_def();
    let high = two_rooms(1.5, 4.0);
    assert_eq!(next_hop(&high, 0, 1, Pass::Walk(d.tuning())), None);
    assert_eq!(
        next_hop(&high, 0, 1, Pass::Fly { height: d.height }),
        Some(1)
    );
    // And the body really flies over the ledge.
    let mut b = Body::spawn(&high, Vec2::new(2.0, 2.0), d.radius, d.height).unwrap();
    for _ in 0..120 {
        step_flyer(&high, &mut b, Vec3::X, d.speed, 2.5, DT);
    }
    assert_eq!(b.sector, 1);
    assert!(b.pos.z >= 1.5 + 0.3 - 1e-4, "{}", b.pos.z);
}

#[test]
fn drone_blocked_by_low_opening() {
    let d = drone_def();
    let fly = Pass::Fly { height: d.height };
    // The far room is lower than the drone is tall.
    let low = two_rooms(0.0, 0.5);
    assert_eq!(next_hop(&low, 0, 1, fly), None);
    // A slit: ceiling 3 on one side, floor 2.8 on the other; each side alone is tall enough.
    let slit = two_rooms(2.8, 5.0);
    assert_eq!(next_hop(&slit, 0, 1, fly), None);
    for map in [&low, &slit] {
        let mut b = Body::spawn(map, Vec2::new(2.0, 2.0), d.radius, d.height).unwrap();
        for _ in 0..120 {
            step_flyer(map, &mut b, Vec3::X, d.speed, 2.5, DT);
        }
        assert_eq!(b.sector, 0, "stopped at {}", b.pos);
    }
}

#[test]
fn drone_slides_along_a_too_low_opening() {
    let d = drone_def();
    for map in [two_rooms(0.0, 0.5), two_rooms(2.8, 5.0)] {
        let mut b = Body::spawn(&map, Vec2::new(3.0, 1.0), d.radius, d.height).unwrap();
        let wish = Vec3::new(1.0, 1.0, 0.0);
        for _ in 0..60 {
            step_flyer(&map, &mut b, wish, d.speed, 2.5, DT);
        }
        assert_eq!(b.sector, 0);
        assert!(b.pos.y > 2.5, "stuck at {}", b.pos);
    }
}

#[test]
fn flyer_hovers_without_gravity() {
    let map = two_rooms(0.0, 4.0);
    let d = drone_def();
    let mut b = Body::spawn(&map, Vec2::new(2.0, 2.0), d.radius, d.height).unwrap();
    for _ in 0..120 {
        step_flyer(&map, &mut b, Vec3::ZERO, d.speed, 2.5, DT);
    }
    assert!(
        (b.pos.z - 2.4).abs() < 1e-4,
        "hover 2.5 clipped by the 3 m ceiling: {}",
        b.pos.z
    );
    assert!(b.vel.z.abs() < 1e-3 && !b.on_ground);
}

#[test]
fn flyer_in_a_squat_room_sits_at_the_floor_clearance() {
    // Ceiling 0.8: the range [0.3, 0.2] is empty, so it sits at the ceiling-clipped height.
    let map = two_rooms(0.0, 0.8);
    let d = drone_def();
    let mut b = Body::spawn(&map, Vec2::new(6.0, 2.0), d.radius, d.height).unwrap();
    step_flyer(&map, &mut b, Vec3::ZERO, d.speed, 2.5, DT);
    assert!((b.pos.z - 0.2).abs() < 1e-4, "{}", b.pos.z);
}

#[test]
fn drone_spawns_at_hover_height() {
    let mut map = two_rooms(0.0, 4.0);
    spawn_kind(&mut map, ActorKind::Drone, 2.0, 2.0, 0.0, true);
    let c = Combat::spawn(&map, &defs(), 1);
    assert!((c.actors[0].body.pos.z - 2.4).abs() < 1e-4);
}

#[test]
fn drone_ignores_lift() {
    let d = defs();
    let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
    // Asleep over the lift, facing away from the player.
    spawn_kind(&mut map, ActorKind::Drone, 5.0, 2.0, 0.0, true);
    let mut mech = Mechanics::new(&mut map);
    let c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    // Mechanics alone never moves a flyer, however the floor under it moves.
    let mut body = c.actors[0].body;
    body.pos.z = 3.0;
    body.on_ground = false;
    let mut bodies = [body];
    mech.toggle(0);
    for _ in 0..30 {
        mech.tick(&mut map, &mut bodies, DT);
        assert_eq!(bodies[0].pos.z, 3.0);
    }
    mech.toggle(0); // reverse back down
    let mut c = Combat::spawn(&map, &d, 1);
    let mut last = c.actors[0].body.pos.z;
    for _ in 0..240 {
        p.tick(&mut c, &mut map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
        let z = c.actors[0].body.pos.z;
        assert!(
            (z - last).abs() <= 6.0 * DT + 1e-4,
            "no jump from {last} to {z}"
        );
        last = z;
    }
    let floor = map.sectors[1].floor_z;
    assert!(
        (last - (floor + 2.5)).abs() < 1e-3,
        "hovers over the lift: {last} over {floor}"
    );
}

/// An awake drone in the combat room, 8 m from a player in plain sight.
fn dogfight() -> (Map, Combat, Player, Defs) {
    let d = defs();
    let mut map = crate::fixtures::combat_room();
    spawn_kind(
        &mut map,
        ActorKind::Drone,
        6.0,
        1.5,
        std::f32::consts::PI,
        false,
    );
    let c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    (map, c, p, d)
}

#[test]
fn drone_swoops_then_climbs() {
    let (mut map, mut c, mut p, d) = dogfight();
    let mut dived = None;
    let mut fired_at = None;
    let mut climbed = false;
    for i in 0..600 {
        let ev = p.tick(&mut c, &mut map, &d);
        p.vitals.health = crate::health::Health::new(1000); // stay alive for the test
        let a = &c.actors[0];
        let eye_target = p.eye().z - 0.3;
        if matches!(a.state, AiState::Attack { .. }) && (a.body.pos.z - eye_target).abs() < 0.05 {
            dived = Some(i);
        }
        if ev.contains(&CombatEvent::ActorFired { actor: 0 }) && fired_at.is_none() {
            fired_at = Some(i);
            assert!(
                (a.body.pos.z - eye_target).abs() < 0.3,
                "fires from the dive: z {}",
                a.body.pos.z
            );
        }
        if fired_at.is_some() && (a.body.pos.z - 2.4).abs() < 0.05 {
            climbed = true;
            break;
        }
    }
    assert!(dived.is_some(), "reached the swoop height in Attack");
    assert!(
        fired_at.is_some() && climbed,
        "fired, then climbed back to hover"
    );
}

#[test]
fn killed_drone_falls_to_the_floor() {
    let (mut map, mut c, mut p, d) = dogfight();
    p.tick(&mut c, &mut map, &d);
    assert!(c.actors[0].body.pos.z > 2.0);
    c.damage_actor(&d, 0, 1000, Shooter::Player);
    let mut last = c.actors[0].body.pos.z;
    let mut fell = false;
    for _ in 0..180 {
        p.tick(&mut c, &mut map, &d);
        let z = c.actors[0].body.pos.z;
        assert!(z <= last + 1e-4, "never rises: {last} -> {z}");
        fell |= z < last - 0.01 && z > 0.0;
        last = z;
    }
    assert!(fell, "it dropped over several ticks, not in one snap");
    assert_eq!(c.actors[0].body.pos.z, 0.0);
}

/// `next_hop` routes the Drone from room `from` to the other, and the body gets there.
fn flies_through(map: &Map, from: usize, x: f32, dir: Vec3) {
    let d = drone_def();
    let fly = Pass::Fly { height: d.height };
    assert_eq!(next_hop(map, from, 1 - from, fly), Some(1 - from));
    let mut b = Body::spawn(map, Vec2::new(x, 2.0), d.radius, d.height).unwrap();
    for _ in 0..240 {
        step_flyer(map, &mut b, dir, d.speed, 2.5, DT);
    }
    assert_eq!(b.sector, 1 - from, "stuck at {}", b.pos);
}

#[test]
fn drone_enters_low_ceiling_neighbour_same_floor() {
    // Opening 0.8 (0.6 body): the neighbour's ceiling is only 0.8 above the shared floor.
    flies_through(&two_rooms(0.0, 0.8), 0, 2.0, Vec3::X);
}

#[test]
fn drone_enters_low_neighbour_below_a_high_floor() {
    // Starts over floor 2.3; the neighbour's ceiling is 3.0: opening 0.7.
    flies_through(&two_rooms(2.3, 5.0), 1, 6.0, Vec3::NEG_X);
}
