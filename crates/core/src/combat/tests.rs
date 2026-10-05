use super::*;
use crate::defs::WeaponId;
use crate::fixtures::{combat_room, defs, door_rooms, lift_shaft};
use crate::health::PLAYER_MAX_HEALTH;
use crate::map::{ActorKind, ActorSpawn};
use crate::mechanics::{Mechanics, Motion};
use crate::movement::MoveInput;
use crate::vitals::Vitals;
use crate::weapons::WeaponEvent;
use std::f32::consts::PI;

pub(super) const DT: f32 = 1.0 / 60.0;

pub(super) fn spawn_at(map: &mut Map, x: f32, y: f32, angle: f32, asleep: bool) {
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(x, y),
        angle,
        asleep,
    });
}

/// A standing player at `(x, y)` with full health and no armour.
pub(super) struct Player {
    pub(super) body: Body,
    pub(super) vitals: Vitals,
}

impl Player {
    pub(super) fn at(map: &Map, x: f32, y: f32) -> Player {
        Player {
            body: Body::spawn(map, Vec2::new(x, y), 0.35, 1.8).unwrap(),
            vitals: Vitals::new(),
        }
    }

    pub(super) fn eye(&self) -> Vec3 {
        self.body.pos + Vec3::Z * 1.6
    }

    pub(super) fn tick(&mut self, c: &mut Combat, map: &Map, d: &Defs) -> Vec<CombatEvent> {
        let eye = self.eye();
        let mut t = PlayerTarget {
            body: &mut self.body,
            vitals: &mut self.vitals,
            eye,
        };
        c.tick(map, d, &mut t, DT)
    }

    pub(super) fn shoot(
        &self,
        c: &mut Combat,
        map: &Map,
        d: &Defs,
        weapon: WeaponId,
        dirs: Vec<Vec3>,
    ) -> Vec<CombatEvent> {
        let ev = WeaponEvent::Fire { weapon, dirs };
        c.player_attack(map, d, self.eye(), self.body.sector, &ev)
    }

    /// Unit direction from the eye to actor `i`'s chest.
    pub(super) fn aim_at(&self, c: &Combat, i: usize) -> Vec3 {
        let a = &c.actors[i];
        (a.body.pos + Vec3::Z * 0.6 * a.body.height - self.eye()).normalize()
    }
}

/// One mover tick the way the game runs it: the player plus every living actor.
pub(super) fn mech_tick(mech: &mut Mechanics, map: &mut Map, c: &mut Combat, p: &mut Player) {
    let (idx, living) = c.living_bodies();
    let mut bodies = vec![p.body];
    bodies.extend(living);
    mech.tick(map, &mut bodies, DT);
    p.body = bodies[0];
    c.write_back(&idx, &bodies[1..]);
}

pub(super) fn count(ev: &[CombatEvent], f: impl Fn(&CombatEvent) -> bool) -> usize {
    ev.iter().filter(|e| f(e)).count()
}

#[test]
fn level_seed_is_fnv1a() {
    assert_eq!(level_seed(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(level_seed("a"), 0xaf63_dc4c_8601_ec8c);
    assert_ne!(level_seed("combat arena"), level_seed("combat arenb"));
}

#[test]
fn spawn_builds_actors_and_skips_outside_spawns() {
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.5, 0.0, true);
    spawn_at(&mut map, 100.0, 100.0, 0.0, true);
    spawn_at(&mut map, 14.0, 4.0, 0.0, false);
    let c = Combat::spawn(&map, &defs(), 1);
    assert_eq!(c.actors.len(), 2);
    assert_eq!(c.actors[0].state, AiState::Sleep);
    assert_eq!(c.actors[1].state, AiState::Alert { t: 0.0 });
    assert_eq!(c.actors[1].body.sector, 4);
    assert!(c.projectiles.is_empty());
}

#[test]
fn pistol_kills_grunt_in_three_shots() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.5, PI, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    for shot in 1..=3 {
        let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir]);
        assert!(ev.contains(&CombatEvent::ActorHurt {
            actor: 0,
            amount: 12
        }));
        let killed = ev.contains(&CombatEvent::ActorKilled(0));
        assert_eq!(killed, shot == 3, "shot {shot}: {ev:?}");
        assert_eq!(count(&ev, |e| matches!(e, CombatEvent::Impact { .. })), 0);
    }
    assert!(!c.actors[0].alive());
    // A fourth shot passes through the corpse and hits the wall behind it.
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir]);
    assert_eq!(count(&ev, |e| matches!(e, CombatEvent::Impact { .. })), 1);
    assert_eq!(
        count(&ev, |e| matches!(
            e,
            CombatEvent::ActorHurt { .. } | CombatEvent::ActorKilled(_)
        )),
        0
    );
}

#[test]
fn pellet_stops_at_first_body() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 5.5, 1.5, PI, false);
    spawn_at(&mut map, 7.0, 1.5, PI, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Shotgun, vec![dir, dir]);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, CombatEvent::ActorHurt { .. }))
            .collect::<Vec<_>>(),
        vec![&CombatEvent::ActorHurt {
            actor: 0,
            amount: 16
        }],
        "two pellets summed into one hurt on the front grunt"
    );
    assert_eq!(c.actors[1].health.hp, c.actors[1].health.max);
}

#[test]
fn shotgun_pellets_split_between_two_grunts() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 0.8, PI, false);
    spawn_at(&mut map, 6.0, 2.4, PI, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    let (a, b) = (p.aim_at(&c, 0), p.aim_at(&c, 1));
    let wall = Vec3::new(0.0, -1.0, 0.0);
    // Interleaved so the event order cannot simply follow the pellet order.
    let dirs = vec![b, a, b, a, wall, a];
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Shotgun, dirs);
    let hurts: Vec<_> = ev
        .iter()
        .filter(|e| matches!(e, CombatEvent::ActorHurt { .. }))
        .collect();
    assert_eq!(
        hurts,
        vec![
            &CombatEvent::ActorHurt {
                actor: 0,
                amount: 24
            },
            &CombatEvent::ActorHurt {
                actor: 1,
                amount: 16
            },
        ]
    );
    let impacts: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            CombatEvent::Impact { point, normal, .. } => Some((*point, *normal)),
            _ => None,
        })
        .collect();
    assert_eq!(impacts.len(), 1);
    assert!(impacts[0].0.y.abs() < 1e-3, "south wall at y=0");
    assert!((impacts[0].1 - Vec3::Y).length() < 1e-3);
}

#[test]
fn kick_hits_only_within_reach() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 2.5, 1.5, PI, false);
    spawn_at(&mut map, 6.5, 6.5, PI, false);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    let kick = WeaponEvent::Kick {
        dir: p.aim_at(&c, 0),
    };
    let ev = c.player_attack(&map, &d, p.eye(), p.body.sector, &kick);
    assert!(ev.contains(&CombatEvent::ActorHurt {
        actor: 0,
        amount: 12
    }));
    // Out of reach: no hit, and nothing to strike within 1.4 m.
    let far = WeaponEvent::Kick {
        dir: p.aim_at(&c, 1),
    };
    let ev = c.player_attack(&map, &d, p.eye(), p.body.sector, &far);
    assert!(ev.is_empty(), "{ev:?}");
    // Non-attack events do nothing.
    for e in [WeaponEvent::DryFire, WeaponEvent::ReloadStart] {
        assert!(c.player_attack(&map, &d, p.eye(), 0, &e).is_empty());
    }
}

#[test]
fn hit_wakes_sleeper_but_kill_reports_only_killed() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.0, 0.0, true);
    spawn_at(&mut map, 6.0, 2.4, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 1.5);
    let dir = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir]);
    assert_eq!(count(&ev, |e| *e == CombatEvent::ActorWoke(0)), 1, "{ev:?}");
    // The noise wakes the other sleeper too.
    assert_eq!(count(&ev, |e| *e == CombatEvent::ActorWoke(1)), 1, "{ev:?}");

    let mut c = Combat::spawn(&map, &d, 1);
    let dir = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir; 3]);
    assert!(ev.contains(&CombatEvent::ActorKilled(0)));
    assert!(!ev.contains(&CombatEvent::ActorWoke(0)), "{ev:?}");
}

#[test]
fn actor_killed_this_tick_does_not_fire() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.5, PI, false);
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 1.5, 1.5);

    // Control: left alone, it fires this tick.
    let mut control = c.clone();
    let ev = p.clone_tick(&mut control, &map, &d);
    assert!(ev.contains(&CombatEvent::ActorFired { actor: 0 }), "{ev:?}");
    assert_eq!(control.projectiles.len(), 1);

    let dir = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir; 3]);
    assert!(ev.contains(&CombatEvent::ActorKilled(0)));
    for _ in 0..120 {
        let ev = p.tick(&mut c, &map, &d);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, CombatEvent::ActorFired { .. })),
            "{ev:?}"
        );
    }
    assert!(c.projectiles.is_empty());
    assert_eq!(c.actors[0].state, AiState::Dead);
}

impl Player {
    /// `tick` on a throwaway copy of the player.
    fn clone_tick(&self, c: &mut Combat, map: &Map, d: &Defs) -> Vec<CombatEvent> {
        let mut copy = Player {
            body: self.body,
            vitals: self.vitals,
        };
        copy.tick(c, map, d)
    }
}

#[test]
fn gunfire_wakes_nearby_but_not_behind_closed_door() {
    let d = defs();
    let mut authored = combat_room();
    // Facing away from the player so only noise can wake them.
    spawn_at(&mut authored, 7.0, 7.0, PI / 2.0, true); // main room, ~8 m away
    spawn_at(&mut authored, 14.5, 4.0, 0.0, true); // room B, behind the door
    let mut closed = authored.clone();
    Mechanics::new(&mut closed);
    let p = Player::at(&authored, 1.5, 1.5);
    let wall = Vec3::new(0.0, -1.0, 0.0);

    let mut c = Combat::spawn(&closed, &d, 1);
    let ev = p.shoot(&mut c, &closed, &d, WeaponId::Pistol, vec![wall]);
    assert!(ev.contains(&CombatEvent::ActorWoke(0)), "{ev:?}");
    assert!(!ev.contains(&CombatEvent::ActorWoke(1)), "{ev:?}");
    assert_eq!(c.actors[0].state, AiState::Alert { t: 0.0 });
    assert_eq!(c.actors[1].state, AiState::Sleep);

    // Door open: the same shot carries into room B.
    let mut c = Combat::spawn(&authored, &d, 1);
    let ev = p.shoot(&mut c, &authored, &d, WeaponId::Pistol, vec![wall]);
    assert!(ev.contains(&CombatEvent::ActorWoke(0)), "{ev:?}");
    assert!(ev.contains(&CombatEvent::ActorWoke(1)), "{ev:?}");

    // A kick is quiet: 4 m does not reach a sleeper 8 m away.
    let mut c = Combat::spawn(&authored, &d, 1);
    let kick = WeaponEvent::Kick { dir: wall };
    let ev = c.player_attack(&authored, &d, p.eye(), p.body.sector, &kick);
    assert!(!ev.iter().any(|e| matches!(e, CombatEvent::ActorWoke(_))));
    assert_eq!(
        c.make_noise(&authored, p.eye(), 0, 4.0),
        Vec::<usize>::new()
    );
    assert_eq!(c.make_noise(&authored, p.eye(), 0, 9.0), vec![0]);
    assert_eq!(
        c.make_noise(&authored, p.eye(), 0, 9.0),
        Vec::<usize>::new(),
        "already awake"
    );
}

#[test]
fn bolt_damages_player_and_reports_direction() {
    let d = defs();
    let mut map = combat_room();
    spawn_at(&mut map, 6.5, 1.5, PI, false);
    let mut c = Combat::spawn(&map, &d, 7);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 1.5, 1.5);
    let mut log = Vec::new();
    for _ in 0..120 {
        log.extend(p.tick(&mut c, &map, &d));
        if log
            .iter()
            .any(|e| matches!(e, CombatEvent::PlayerHurt { .. }))
        {
            break;
        }
    }
    let fired = log
        .iter()
        .position(|e| *e == CombatEvent::ActorFired { actor: 0 })
        .expect("grunt fired");
    let (at, amount, from) = log
        .iter()
        .enumerate()
        .find_map(|(i, e)| match e {
            CombatEvent::PlayerHurt { amount, from } => Some((i, *amount, *from)),
            _ => None,
        })
        .expect("bolt hit the player");
    assert!(fired < at);
    assert_eq!(
        amount,
        match d.enemy(ActorKind::Grunt).attack {
            EnemyAttack::Bolts { proj, .. } => proj.damage,
            _ => unreachable!("the Grunt shoots bolts"),
        }
    );
    assert_eq!(p.vitals.health.hp, PLAYER_MAX_HEALTH - amount);
    let came_from = (from - p.body.pos).truncate().normalize();
    assert!(
        came_from.angle_to(Vec2::X).abs() < 30f32.to_radians(),
        "from {from} reads as {came_from}"
    );
    // The first bolt (id 0) left play; the burst's second one is still in flight.
    assert!(log[at..].contains(&CombatEvent::ProjectileGone(0)));
    assert!(c.projectiles.iter().all(|b| b.id != 0));
}

/// A bolt from actor 0 at `pos` flying west at 15 m/s.
fn bolt(c: &mut Combat, map: &Map, pos: Vec3) {
    let id = c.next_id;
    c.next_id += 1;
    c.projectiles.push(Projectile {
        id,
        pos,
        prev: pos,
        vel: Vec3::new(-15.0, 0.0, 0.0),
        sector: map.find_sector(pos.truncate(), None).unwrap(),
        radius: 0.12,
        damage: 8,
        owner: Shooter::Actor(0),
        targets: Targets::Player,
        life: 4.0,
        gravity: 0.0,
        bounce: None,
        remote: false,
        splash: None,
        resting: false,
    });
}

#[test]
fn player_killed_reported_once() {
    let d = defs();
    let mut map = combat_room();
    // Asleep and facing away: it never fires on its own.
    spawn_at(&mut map, 7.5, 7.0, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    p.vitals.health.hp = 10;
    // Two bolts arriving in the same tick, then more after the player is dead.
    bolt(&mut c, &map, Vec3::new(2.5, 1.5, 1.0));
    bolt(&mut c, &map, Vec3::new(2.5, 1.6, 1.2));
    bolt(&mut c, &map, Vec3::new(4.0, 1.5, 1.0));
    let mut log = Vec::new();
    for _ in 0..60 {
        log.extend(p.tick(&mut c, &map, &d));
    }
    assert_eq!(count(&log, |e| *e == CombatEvent::PlayerKilled), 1);
    assert_eq!(
        count(&log, |e| matches!(e, CombatEvent::PlayerHurt { .. })),
        2,
        "a dead player takes no more hurt: {log:?}"
    );
    assert_eq!(
        count(&log, |e| matches!(e, CombatEvent::ProjectileGone(_))),
        3
    );
    assert!(!p.vitals.health.alive());
}

#[test]
fn grunt_rides_lift() {
    let d = defs();
    let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
    // On the lift, facing away from the player.
    spawn_at(&mut map, 5.0, 2.0, 0.0, true);
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    mech.toggle(0);
    for _ in 0..120 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
    }
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
    assert!((c.actors[0].body.pos.z - 2.0).abs() < 1e-4);
    assert_eq!(c.actors[0].state, AiState::Sleep);
}

/// Door rooms with the door opened, a grunt asleep in the doorway (facing away from the
/// player in room A).
fn doorway() -> (Map, Mechanics, Combat, Player, Defs) {
    let d = defs();
    let mut map = door_rooms("(kind: Door)", "");
    spawn_at(&mut map, 4.25, 2.0, 0.0, true);
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    mech.toggle(0);
    for _ in 0..120 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
    }
    assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
    (map, mech, c, p, d)
}

#[test]
fn door_reverses_on_grunt() {
    let (mut map, mut mech, mut c, mut p, d) = doorway();
    let top = c.actors[0].body.pos.z + c.actors[0].body.height;
    mech.toggle(0);
    let mut reversed = false;
    for _ in 0..180 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
        assert!(
            map.sectors[1].ceil_z >= top - 1e-3,
            "door crushed the grunt"
        );
        reversed |= mech.movers[0].motion == Motion::ToEnd;
    }
    assert!(reversed);
}

#[test]
fn corpse_does_not_hold_door_open() {
    let (mut map, mut mech, mut c, mut p, d) = doorway();
    let dir = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir; 3]);
    assert!(ev.contains(&CombatEvent::ActorKilled(0)), "{ev:?}");
    assert!(c.living_bodies().0.is_empty());
    mech.toggle(0);
    for _ in 0..180 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
    }
    assert_eq!(mech.movers[0].motion, Motion::AtStart);
    assert_eq!(map.sectors[1].ceil_z, map.sectors[1].floor_z);
    assert_eq!(c.actors[0].body.pos.z, map.sectors[1].floor_z);
}

#[test]
fn corpse_snaps_to_floor() {
    let d = defs();
    let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
    spawn_at(&mut map, 5.0, 2.0, 0.0, true);
    let mut mech = Mechanics::new(&mut map);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.0, 2.0);
    let dir = p.aim_at(&c, 0);
    p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir; 3]);
    assert!(!c.actors[0].alive());
    mech.toggle(0);
    for _ in 0..120 {
        p.tick(&mut c, &map, &d);
        mech_tick(&mut mech, &mut map, &mut c, &mut p);
    }
    assert_eq!(
        c.actors[0].body.pos.z, 2.0,
        "the corpse follows the lift floor"
    );
}

#[test]
fn player_and_grunt_do_not_overlap() {
    let d = defs();
    let mut map = combat_room();
    // Overlapping the player, asleep and facing away (it never moves on its own).
    spawn_at(&mut map, 1.9, 1.5, 0.0, true);
    // Overlapping a player pinned against the west wall.
    spawn_at(&mut map, 0.6, 6.0, 0.0, true);
    let mut c = Combat::spawn(&map, &d, 1);
    let mut p = Player::at(&map, 1.5, 1.5);
    let mut q = Player::at(&map, 0.36, 6.0);
    let reach = |a: &Body, b: &Body| a.radius + b.radius;
    p.tick(&mut c, &map, &d);
    let gap = c.actors[0]
        .body
        .pos
        .truncate()
        .distance(p.body.pos.truncate());
    assert!(gap >= reach(&c.actors[0].body, &p.body) - 1e-3, "gap {gap}");
    // Each was pushed half of the 0.3 m overlap, straight apart.
    assert!((p.body.pos.x - 1.35).abs() < 1e-3, "{}", p.body.pos);
    assert!((c.actors[0].body.pos.x - 2.05).abs() < 1e-3);
    for _ in 0..120 {
        q.tick(&mut c, &map, &d);
    }
    let gap = c.actors[1]
        .body
        .pos
        .truncate()
        .distance(q.body.pos.truncate());
    assert!(gap >= reach(&c.actors[1].body, &q.body) - 1e-3, "gap {gap}");
    assert!(q.body.pos.x >= 0.35 - 1e-3, "the wall holds the player");
}

#[test]
fn combat_is_deterministic_for_a_seed() {
    let run = |seed: u64| {
        let d = defs();
        let mut map = combat_room();
        spawn_at(&mut map, 6.5, 1.5, PI, false);
        spawn_at(&mut map, 7.0, 6.5, PI, false);
        spawn_at(&mut map, 14.5, 4.0, PI, true);
        let mut c = Combat::spawn(&map, &d, seed);
        let mut p = Player::at(&map, 1.5, 1.5);
        p.vitals.health.hp = 10_000;
        p.vitals.health.max = 10_000;
        let mut log = Vec::new();
        for i in 0..600 {
            if i % 40 == 0 {
                let dir = p.aim_at(&c, 1);
                log.extend(p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![dir]));
            }
            log.extend(p.tick(&mut c, &map, &d));
        }
        (log, c.actors, p.body, p.vitals)
    };
    let a = run(42);
    assert_eq!(a, run(42));
    assert_ne!(a.0, run(43).0, "another seed plays out differently");
    assert!(
        a.0.iter()
            .any(|e| matches!(e, CombatEvent::ActorFired { .. }))
    );
}

pub(super) fn spawn_kind(map: &mut Map, kind: ActorKind, x: f32, y: f32, angle: f32, asleep: bool) {
    map.actors.push(ActorSpawn {
        kind,
        pos: Vec2::new(x, y),
        angle,
        asleep,
    });
}

/// Shipped defs with the Enforcer planted, so its line of fire is predictable.
fn standing_enforcer() -> Defs {
    let mut d = defs();
    for e in &mut d.enemies {
        if e.kind == ActorKind::Enforcer {
            e.strafe = false;
        }
    }
    d
}

#[test]
fn bolt_spawns_at_muzzle_offset() {
    let mut map = combat_room();
    spawn_at(&mut map, 6.0, 1.5, PI, false);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 1.5, 1.5);
    let def = d.enemy(ActorKind::Grunt);
    let (fwd, side, up) = def.muzzle;
    for _ in 0..120 {
        let ev = p.tick(&mut c, &map, &d);
        if count(&ev, |e| matches!(e, CombatEvent::ActorFired { .. })) > 0 {
            let a = &c.actors[0];
            let b = &c.projectiles[0];
            // The bolt starts at the def offset rotated by the heading, not at chest height.
            let (sin, cos) = a.angle.sin_cos();
            let want = a.body.pos + Vec3::new(fwd * cos - side * sin, fwd * sin + side * cos, up);
            assert!(b.prev.distance(want) < 1e-4, "{:?} vs {want:?}", b.prev);
            assert!((b.prev.z - a.body.pos.z - up).abs() < 1e-4);
            assert!(b.prev.z - a.body.pos.z > 0.6 * a.body.height);
            return;
        }
    }
    panic!("the Grunt never fired");
}

#[test]
fn enforcer_hitscan_hurts_player() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Enforcer, 2.0, 1.5, 0.0, false);
    let d = standing_enforcer();
    let mut c = Combat::spawn(&map, &d, 7);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 1.5);
    let mut hurt_total = 0;
    let (mut volleys, mut impacts) = (0, 0);
    for _ in 0..900 {
        let before = p.vitals.health.hp;
        let ev = p.tick(&mut c, &map, &d);
        volleys += count(&ev, |e| matches!(e, CombatEvent::ActorFired { .. }));
        impacts += count(&ev, |e| matches!(e, CombatEvent::Impact { .. }));
        for e in &ev {
            if let CombatEvent::PlayerHurt { amount, from } = e {
                // One event per volley: whole pellets of 5, at most 6 of them.
                assert!(amount % 5 == 0 && (5..=30).contains(amount), "{amount}");
                assert_eq!(*from, c.actors[0].muzzle());
                if p.vitals.health.alive() {
                    assert_eq!(before - p.vitals.health.hp, *amount);
                }
                hurt_total += amount;
            }
        }
        assert!(c.projectiles.is_empty(), "hitscan spawns no bolts");
        if !p.vitals.health.alive() {
            break;
        }
    }
    assert!(volleys >= 2, "{volleys} volleys");
    assert!(hurt_total > 0, "no pellet ever hit the player");
    assert!(impacts > 0, "pellets that miss hit the wall");
}

#[test]
fn hitscan_pellets_stop_at_other_bodies() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Enforcer, 2.0, 1.5, 0.0, false);
    spawn_kind(&mut map, ActorKind::Barrel, 3.5, 1.5, 0.0, true);
    let mut d = standing_enforcer();
    // A tall crate: the muzzle is above a normal barrel's top.
    for e in &mut d.enemies {
        if e.kind == ActorKind::Barrel {
            e.height = 2.0;
        }
    }
    let mut c = Combat::spawn(&map, &d, 3);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 7.0, 1.5);
    // Actors walk through each other, so only the first volley, fired from behind the
    // crate, is guaranteed to be blocked.
    let ev = p.tick(&mut c, &map, &d);
    assert!(ev.contains(&CombatEvent::ActorFired { actor: 0 }), "{ev:?}");
    assert_eq!(
        count(&ev, |e| matches!(e, CombatEvent::PlayerHurt { .. })),
        0
    );
    assert_eq!(p.vitals.health.hp, PLAYER_MAX_HEALTH);
    assert_eq!(c.actors[1].health.hp, d.enemy(ActorKind::Barrel).health);
}

#[test]
fn slasher_lunges_and_hits() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Slasher, 2.0, 1.5, 0.0, false);
    let d = defs();
    let def = d.enemy(ActorKind::Slasher);
    let mut c = Combat::spawn(&map, &d, 5);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 3.4, 1.5);
    let mut top_speed = 0.0_f32;
    for _ in 0..120 {
        let ev = p.tick(&mut c, &map, &d);
        if matches!(c.actors[0].state, AiState::Attack { .. }) {
            top_speed = top_speed.max(c.actors[0].body.vel.truncate().length());
        }
        if let Some(CombatEvent::PlayerHurt { amount, from }) = ev
            .iter()
            .find(|e| matches!(e, CombatEvent::PlayerHurt { .. }))
        {
            assert_eq!(*amount, 18);
            let eye = c.actors[0].eye();
            assert!(
                (from.z - eye.z).abs() < 1e-4 && from.truncate().distance(eye.truncate()) < 0.3
            );
            assert!(top_speed > def.speed, "lunge speed {top_speed}");
            return;
        }
    }
    panic!("the Slasher never hit");
}

#[test]
fn slasher_misses_when_player_backs_off() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Slasher, 2.0, 1.5, 0.0, false);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 5);
    c.actors[0].state = AiState::Chase;
    let mut p = Player::at(&map, 3.4, 1.5);
    let mut swung = false;
    for _ in 0..30 {
        if matches!(c.actors[0].state, AiState::Attack { .. }) && !swung {
            // The player leaps clear the moment the lunge starts.
            p.body.pos.x = 7.5;
            swung = true;
        }
        let ev = p.tick(&mut c, &map, &d);
        assert_eq!(
            count(&ev, |e| matches!(e, CombatEvent::PlayerHurt { .. })),
            0
        );
        if swung && !matches!(c.actors[0].state, AiState::Attack { .. }) {
            break;
        }
    }
    assert!(swung, "the Slasher never attacked");
    assert_eq!(p.vitals.health.hp, PLAYER_MAX_HEALTH);
    assert!(c.actors[0].refire > 0.0, "a miss still costs the refire");
}

#[test]
fn barrel_never_thinks() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Barrel, 4.0, 1.5, 0.0, false);
    let d = defs();
    let def = d.enemy(ActorKind::Barrel);
    let mut c = Combat::spawn(&map, &d, 2);
    let start = c.actors[0].clone();
    let mut p = Player::at(&map, 2.0, 1.5);
    for _ in 0..300 {
        assert!(p.tick(&mut c, &map, &d).is_empty());
    }
    assert!(c.make_noise(&map, p.eye(), p.body.sector, 30.0).is_empty());
    assert_eq!(c.actors[0], start, "no state change, no movement");
    assert_eq!(c.actors[0].state, AiState::Sleep);
    // It is still a body: a trace hits it, damage lands without pain or waking, a kill goes
    // through Dying to Dead.
    let aim = p.aim_at(&c, 0);
    let ev = p.shoot(&mut c, &map, &d, WeaponId::Pistol, vec![aim]);
    assert!(
        ev.iter()
            .any(|e| matches!(e, CombatEvent::ActorHurt { .. }))
    );
    assert!(c.actors[0].health.hp < def.health);
    assert_eq!(c.actors[0].state, AiState::Sleep, "no wake, no pain");
    let ev = c.damage_actor(&d, 0, 1000);
    assert!(ev.contains(&CombatEvent::ActorKilled(0)));
    for _ in 0..30 {
        p.tick(&mut c, &map, &d);
    }
    assert_eq!(c.actors[0].state, AiState::Dead);
}

#[test]
fn player_cannot_shove_a_barrel() {
    let mut map = combat_room();
    spawn_kind(&mut map, ActorKind::Barrel, 4.0, 1.5, 0.0, true);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 2);
    let barrel_at = c.actors[0].body.pos;
    let mut p = Player::at(&map, 2.0, 1.5);
    let walk = MoveInput {
        wish: Vec2::X,
        ..MoveInput::default()
    };
    for _ in 0..60 {
        step_player(&map, &mut p.body, &walk, &Tuning::default(), DT);
        p.tick(&mut c, &map, &d);
        let reach = c.actors[0].body.radius + p.body.radius;
        let dist = p.body.pos.truncate().distance(barrel_at.truncate());
        assert!(dist >= reach - 1e-3, "player inside the barrel: {dist}");
    }
    assert_eq!(c.actors[0].body.pos, barrel_at, "the barrel never moves");
    assert!(p.body.pos.x < barrel_at.x, "the player is stopped by it");
}

#[test]
fn muzzle_behind_a_wall_is_clipped_to_the_actor_side() {
    // The pillar (x,y in [3,5]) has its east face at x = 5. A Grunt flush against it,
    // facing it, has its muzzle 0.62 ahead, inside the pillar and past that face.
    let mut map = combat_room();
    spawn_at(&mut map, 5.36, 4.0, PI, false);
    spawn_kind(&mut map, ActorKind::Enforcer, 5.36, 3.5, PI, false);
    let d = defs();
    let mut c = Combat::spawn(&map, &d, 1);
    let p = Player::at(&map, 1.5, 4.0);
    let a = &c.actors[0];
    assert!(a.muzzle().x < 5.0, "setup: the raw muzzle is in the wall");
    let (at, sector) = effective_muzzle(&map, a);
    assert!(at.x >= 5.0, "clipped muzzle {at:?} is on the actor's side");
    assert_eq!(sector, a.body.sector);

    // A bolt starts there and dies on the pillar face.
    let EnemyAttack::Bolts { proj, .. } = d.enemy(ActorKind::Grunt).attack else {
        panic!("the Grunt shoots bolts")
    };
    c.spawn_bolt(&map, 0, Vec3::NEG_X, proj);
    assert!(c.projectiles[0].prev.x >= 5.0);

    // A hitscan volley from the Enforcer cannot reach the player behind the pillar either.
    let e = &c.actors[1];
    let bodies = [p.body, c.actors[0].body, e.body];
    let mut rng = Rng::new(4);
    let v = volley(
        &map,
        e,
        1,
        d.enemy(ActorKind::Enforcer),
        Vec3::NEG_X,
        &bodies,
        |_| true,
        &mut rng,
    );
    assert_eq!(v.player_damage, 0);
    assert!(
        v.impacts.iter().all(|(pt, _, _)| pt.x >= 4.99),
        "{:?}",
        v.impacts
    );
}
