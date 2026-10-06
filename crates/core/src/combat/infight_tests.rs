//! Infighting: enemies that hurt each other turn on each other (Doom rules), and kills carry
//! their killer.

use super::tests::{Player, spawn_kind};
use super::*;
use crate::actors::Target;
use crate::defs::WeaponId;
use crate::fixtures::{combat_room, defs};
use crate::map::ActorKind;
use std::f32::consts::PI;

/// Shipped defs with planted shooters and a Grunt tough enough to survive a volley.
fn infight_defs() -> Defs {
    let mut d = defs();
    for e in &mut d.enemies {
        e.strafe = false;
        if e.kind == ActorKind::Grunt {
            e.health = 200;
        }
    }
    d
}

fn tough(p: &mut Player) {
    p.vitals.health.max = 100_000;
    p.vitals.health.hp = 100_000;
}

#[test]
fn pellets_through_a_grunt_make_it_turn_on_the_enforcer() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Enforcer, 2.0, 1.5, 0.0, false);
    spawn_kind(&mut map, ActorKind::Grunt, 3.5, 1.5, PI, true);
    let d = infight_defs();
    let mut c = Combat::spawn(&map, &d, 3);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 1.5);
    let ev = p.tick(&mut c, &mut map, &d);
    assert!(ev.contains(&CombatEvent::ActorFired { actor: 0 }), "{ev:?}");
    assert!(c.actors[1].health.hp < 200, "the grunt took the pellets");
    assert_eq!(c.actors[1].target, Target::Actor(0));
    assert_eq!(
        c.actors[0].target,
        Target::Player,
        "the enforcer was not hurt"
    );
}

#[test]
fn a_turned_grunt_kills_the_enforcer_and_gets_the_credit() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Enforcer, 2.0, 1.5, 0.0, false);
    spawn_kind(&mut map, ActorKind::Grunt, 3.5, 1.5, PI, true);
    let mut d = infight_defs();
    for e in &mut d.enemies {
        if e.kind == ActorKind::Enforcer {
            e.health = 10;
        }
    }
    let mut c = Combat::spawn(&map, &d, 3);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 1.5);
    tough(&mut p);
    for _ in 0..600 {
        let ev = p.tick(&mut c, &mut map, &d);
        if ev.contains(&CombatEvent::ActorKilled {
            actor: 0,
            by: Shooter::Actor(1),
        }) {
            return;
        }
        assert!(
            !ev.iter()
                .any(|e| matches!(e, CombatEvent::ActorKilled { actor: 0, .. })),
            "{ev:?}"
        );
    }
    panic!("the grunt never killed the enforcer");
}

#[test]
fn same_kind_hurts_but_never_infights() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Grunt, 2.0, 1.5, 0.0, false);
    spawn_kind(&mut map, ActorKind::Grunt, 3.5, 1.5, 0.0, true);
    let d = infight_defs();
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 1.5);
    tough(&mut p);
    for _ in 0..300 {
        p.tick(&mut c, &mut map, &d);
        if c.actors[1].health.hp < 200 {
            assert_eq!(c.actors[1].target, Target::Player);
            return;
        }
    }
    panic!("the back grunt's bolts never hit the front one");
}

#[test]
fn target_dying_mid_burst_returns_to_the_player() {
    // Review focus 3: indices stay stable; a dead target is dropped at once, mid-burst too.
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Grunt, 2.0, 1.5, 0.0, false);
    spawn_kind(&mut map, ActorKind::Enforcer, 6.0, 1.5, PI, true);
    let d = infight_defs();
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].target = Target::Actor(1);
    c.actors[0].state = AiState::Attack { t: 0.1, left: 2 };
    c.actors[1].state = AiState::Dying { t: 0.01 };
    c.actors[1].body.height = crate::actors::CORPSE_HEIGHT;
    let mut p = Player::at(&map, 1.0, 6.5);
    p.tick(&mut c, &mut map, &d);
    assert_eq!(c.actors[0].target, Target::Player);
    assert_eq!(c.actors.len(), 2, "the dead keep their index");
}

#[test]
fn barrels_are_never_targets_and_their_blast_credits_the_killer() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Barrel, 6.0, 1.5, 0.0, true);
    spawn_kind(&mut map, ActorKind::Grunt, 6.0, 2.6, 0.0, true);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    let mut log = p.shoot(&mut c, &mut map, &d, WeaponId::Shotgun, vec![dir; 7]);
    for _ in 0..30 {
        log.extend(p.tick(&mut c, &mut map, &d));
    }
    assert!(log.contains(&CombatEvent::ActorKilled {
        actor: 0,
        by: Shooter::Player
    }));
    assert!(
        log.contains(&CombatEvent::ActorKilled {
            actor: 1,
            by: Shooter::Player
        }),
        "{log:?}"
    );
    assert_eq!(c.actors[1].target, Target::Player);
    // A barrel is never a target: a Grunt hurt "by" one stays on the player.
    let mut c = Combat::spawn(&map, &d, 1);
    c.damage_actor(&d, 1, 1, Shooter::Actor(0));
    assert_eq!(c.actors[1].health.hp, d.enemy(ActorKind::Grunt).health - 1);
    assert_eq!(c.actors[1].target, Target::Player);
}
