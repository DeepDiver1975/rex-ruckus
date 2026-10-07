//! The boss: rocket volleys, a minigun phase below half health, its own-splash immunity, no
//! infighting, and the `on_death` action.

use super::tests::{DT, Player, count, spawn_kind};
use super::*;
use crate::actors::Target;
use crate::difficulty::Difficulty;
use crate::fixtures::{combat_room, defs};
use crate::map::{ActorKind, ActorSpawn, SwitchAction};
use std::f32::consts::PI;

fn boss_room(on_death: Option<SwitchAction>) -> Map {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Boss,
        pos: Vec2::new(6.0, 6.0),
        angle: PI,
        asleep: true,
        skill: Difficulty::Easy,
        on_death,
    });
    map
}

#[test]
fn boss_def_has_rockets_then_a_minigun_below_half() {
    let d = defs();
    let b = d.enemy(ActorKind::Boss);
    assert!(b.boss);
    assert!(matches!(b.attack, EnemyAttack::Bolts { proj, .. } if proj.splash.is_some()));
    assert_eq!(
        (b.phase_for(1.0), b.phase_for(0.51), b.phase_for(0.49)),
        (0, 0, 1)
    );
    let p2 = b.phased(1);
    assert!(matches!(p2.attack, EnemyAttack::Hitscan { .. }));
    assert!(p2.speed > b.speed && p2.attack_refire < b.attack_refire);
    assert_eq!(*b.phased(0), *b, "phase 0 is the def itself");
}

#[test]
fn damage_below_half_changes_phase_once() {
    let map = boss_room(None);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let max = c.actors[0].health.max;
    let ev = c.damage_actor(&d, 0, max / 2 + 1, Shooter::Player);
    assert!(
        ev.contains(&CombatEvent::PhaseChanged { actor: 0, phase: 1 }),
        "{ev:?}"
    );
    let ev = c.damage_actor(&d, 0, 1, Shooter::Player);
    assert!(
        !ev.iter()
            .any(|e| matches!(e, CombatEvent::PhaseChanged { .. }))
    );
    assert_eq!(c.actors[0].phase, 1);
}

#[test]
fn phase_two_fires_hitscan_not_rockets() {
    let mut map = boss_room(None);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let max = c.actors[0].health.max;
    c.damage_actor(&d, 0, max / 2 + 1, Shooter::Player);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 1.5, 6.5);
    p.vitals.health.max = 100_000;
    p.vitals.health.hp = 100_000;
    for _ in 0..300 {
        if p.tick(&mut c, &mut map, &d)
            .contains(&CombatEvent::ActorFired { actor: 0 })
        {
            assert!(c.projectiles.is_empty(), "phase 2 is hitscan");
            return;
        }
    }
    panic!("the boss never fired");
}

#[test]
fn on_death_fires_once_when_dead_not_at_the_kill() {
    // The `on_death` action fires once, on the Dying -> Dead tick, not at the killing blow.
    let mut map = boss_room(Some(SwitchAction::Exit));
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let max = c.actors[0].health.max;
    let kill = c.damage_actor(&d, 0, max, Shooter::Player);
    assert!(kill.contains(&CombatEvent::ActorKilled {
        actor: 0,
        by: Shooter::Player
    }));
    assert!(
        !kill
            .iter()
            .any(|e| matches!(e, CombatEvent::DeathAction(_)))
    );
    let (mut actions, mut first) = (0, None);
    for t in 0..600 {
        let ev = p.tick(&mut c, &mut map, &d);
        let n = count(&ev, |e| *e == CombatEvent::DeathAction(SwitchAction::Exit));
        if n > 0 {
            first.get_or_insert(t);
            assert_eq!(c.actors[0].state, AiState::Dead);
        }
        actions += n;
    }
    assert_eq!(actions, 1);
    let death = d.enemy(ActorKind::Boss).death_time;
    assert!(first.unwrap() as f32 * DT >= death - 2.0 * DT);
}

#[test]
fn boss_ignores_its_own_splash_and_never_retargets() {
    let mut map = boss_room(None);
    spawn_kind(&mut map, ActorKind::Grunt, 2.0, 6.5, 0.0, true);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let hp = c.actors[0].health.hp;
    assert!(c.damage_actor(&d, 0, 50, Shooter::Actor(0)).is_empty());
    assert_eq!(c.actors[0].health.hp, hp);
    c.damage_actor(&d, 0, 5, Shooter::Actor(1));
    assert_eq!(c.actors[0].target, Target::Player);
}

#[test]
fn boss_health_shows_while_the_boss_is_awake() {
    let map = boss_room(None);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    assert_eq!(c.boss_health(), None, "asleep");
    c.actors[0].state = AiState::Chase;
    assert_eq!(c.boss_health(), Some(1.0));
    let max = c.actors[0].health.max;
    c.damage_actor(&d, 0, max / 4, Shooter::Player);
    assert!((c.boss_health().unwrap() - 0.75).abs() < 0.01);
}
