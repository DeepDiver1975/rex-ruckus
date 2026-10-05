//! Combat orchestrator: one deterministic per-tick simulation of player attacks, enemy AI,
//! bolts and their effects. Plain Rust; the game turns the returned [`CombatEvent`]s into
//! sound, sparks and HUD feedback.
//!
//! Per fixed tick the game copies [`Combat::live_bombs`] into the arsenal, ticks it, calls
//! [`Combat::player_attack`] for each weapon event, then [`Combat::tick`], then hands
//! [`Combat::living_bodies`] (plus the player) to `Mechanics::tick` and copies them back with
//! [`Combat::write_back`]. Because player attacks run first, an actor killed by the player this
//! tick never fires.
//!
//! Split into [`player_attack`] (the player's weapon events) and [`blasts`] (the explosion
//! queue).

mod blasts;
mod player_attack;

pub use blasts::{BLAST_NOISE, BLAST_NUDGE, PendingBlast};

use crate::actors::{
    Actor, AiState, Perception, effective_muzzle, hurt, swipe_reaches, think, tuning, volley, wake,
};
use crate::collide::{Body, clip_move, z_range};
use crate::defs::{Defs, EnemyAttack, Locomotion, ProjectileDef};
use crate::explosion::Blast;
use crate::health::DamageOutcome;
use crate::map::{Map, SectorId};
use crate::movement::{Tuning, step_player};
use crate::projectile::{Projectile, ProjectileStep, Shooter, Targets, step_projectile};
use crate::rng::Rng;
use crate::vitals::Vitals;
use blasts::sector_of;
use glam::{Vec2, Vec3};

/// Index of the player in the bodies slice `[player, actor 0, actor 1, ...]`; actor `i` is
/// body `i + 1`.
pub const BODY_PLAYER: usize = 0;

/// A portal carries sound only while its live opening is taller than this (metres).
const NOISE_GAP: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombatEvent {
    /// A non-ignored hit on actor `actor`; `amount` is the total damage dealt this attack.
    ActorHurt {
        actor: usize,
        amount: i32,
    },
    ActorKilled(usize),
    /// The actor left `Sleep` and is still alive.
    ActorWoke(usize),
    /// The actor spawned a bolt this tick.
    ActorFired {
        actor: usize,
    },
    /// `from` is the bolt's position at the start of the tick it hit, i.e. a point on its
    /// incoming line: `from - player` gives the direction the shot came from.
    PlayerHurt {
        amount: i32,
        from: Vec3,
    },
    /// Reported exactly once, on the hit that empties the player's health.
    PlayerKilled,
    /// A shot or projectile hit the world at `point` (in `sector`).
    Impact {
        point: Vec3,
        normal: Vec3,
        sector: SectorId,
    },
    ProjectileGone(u32),
    /// A blast went off (rocket, bomb or barrel).
    Explosion {
        point: Vec3,
        radius: f32,
    },
    /// The player set off their live pipe bombs (once per detonation).
    BombsDetonated,
}

/// The player as combat sees it this tick.
pub struct PlayerTarget<'a> {
    pub body: &'a mut Body,
    /// All player damage goes through `Vitals::damage`, so armour soaks its share.
    pub vitals: &'a mut Vitals,
    pub eye: Vec3,
}

#[derive(Debug, Clone)]
pub struct Combat {
    /// Actor indices are the ones used in every event and in `living_bodies`.
    pub actors: Vec<Actor>,
    pub projectiles: Vec<Projectile>,
    /// Blasts waiting for their fuse, in the order they were queued.
    pub pending_blasts: Vec<PendingBlast>,
    pub rng: Rng,
    next_id: u32,
}

/// A stable 64-bit seed from a level name (FNV-1a; does not depend on the Rust version).
pub fn level_seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Damages the player and reports `PlayerHurt { from }` (and `PlayerKilled` on the fatal hit).
fn hurt_player(player: &mut PlayerTarget, amount: i32, from: Vec3, out: &mut Vec<CombatEvent>) {
    match player.vitals.damage(amount) {
        DamageOutcome::Ignored => {}
        outcome => {
            out.push(CombatEvent::PlayerHurt { amount, from });
            if outcome == DamageOutcome::Killed {
                out.push(CombatEvent::PlayerKilled);
            }
        }
    }
}

/// `ActorWoke(i)` if the actor left `Sleep` and is still alive.
fn woke(i: usize, before: AiState, a: &Actor) -> Option<CombatEvent> {
    (before == AiState::Sleep && a.state != AiState::Sleep && a.alive())
        .then_some(CombatEvent::ActorWoke(i))
}

impl Combat {
    /// Builds the actors from `map.actors`, in order. Spawns outside every sector are skipped
    /// (`validate` reports them), so actor indices may differ from spawn indices only on
    /// invalid maps.
    pub fn spawn(map: &Map, defs: &Defs, seed: u64) -> Combat {
        Combat {
            actors: map
                .actors
                .iter()
                .filter_map(|s| Actor::new(map, defs.enemy(s.kind), s))
                .collect(),
            projectiles: Vec::new(),
            pending_blasts: Vec::new(),
            rng: Rng::new(seed),
            next_id: 0,
        }
    }

    /// A fresh projectile id.
    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Applies `n` damage to actor `i` and reports what happened. An actor with a
    /// `death_splash` (a barrel) that dies queues its own blast, owned by itself, on its
    /// `death_fuse`; `Health` reports the kill only once, so it is queued only once.
    fn damage_actor(&mut self, defs: &Defs, i: usize, n: i32) -> Vec<CombatEvent> {
        let a = &mut self.actors[i];
        let def = defs.enemy(a.kind);
        let before = a.state;
        // Measured before the kill shrinks the body to a corpse.
        let middle = a.body.pos + Vec3::Z * 0.5 * a.body.height;
        let mut out = Vec::new();
        let outcome = hurt(a, def, n, &mut self.rng);
        match outcome {
            DamageOutcome::Ignored => {}
            DamageOutcome::Hurt => out.push(CombatEvent::ActorHurt {
                actor: i,
                amount: n,
            }),
            DamageOutcome::Killed => {
                out.push(CombatEvent::ActorHurt {
                    actor: i,
                    amount: n,
                });
                out.push(CombatEvent::ActorKilled(i));
            }
        }
        out.extend(woke(i, before, a));
        if outcome == DamageOutcome::Killed
            && let Some(splash) = def.death_splash
        {
            let blast = Blast {
                center: middle,
                sector: self.actors[i].body.sector,
                splash,
                owner: Shooter::Actor(i),
            };
            self.queue_blast(def.death_fuse, blast);
        }
        out
    }

    /// A noise of loudness `radius` (metres) at `at` in `sector`. Sound spreads through
    /// portals whose live opening is taller than 0.1 m (so closed doors block it) and wakes
    /// every sleeping actor in a reached sector within `radius` of `at`. Returns the woken
    /// actors in ascending order.
    pub fn make_noise(&mut self, map: &Map, at: Vec3, sector: SectorId, radius: f32) -> Vec<usize> {
        let mut heard = vec![false; map.sectors.len()];
        heard[sector] = true;
        let mut queue = std::collections::VecDeque::from([sector]);
        while let Some(s) = queue.pop_front() {
            let here = &map.sectors[s];
            for n in map.neighbours(s) {
                let there = &map.sectors[n];
                let gap = here.ceil_z.min(there.ceil_z) - here.floor_z.max(there.floor_z);
                if !heard[n] && gap > NOISE_GAP {
                    heard[n] = true;
                    queue.push_back(n);
                }
            }
        }
        let mut woken = Vec::new();
        for (i, a) in self.actors.iter_mut().enumerate() {
            if a.state == AiState::Sleep
                && heard[a.body.sector]
                && a.body.pos.distance(at) <= radius
            {
                let before = a.state;
                wake(a);
                if woke(i, before, a).is_some() {
                    woken.push(i);
                }
            }
        }
        woken
    }

    /// One fixed tick, in this (load-bearing) order:
    /// 1. remember every actor's `prev_pos`;
    /// 2. AI: every actor thinks (dying ones count down); living ones move with the enemy
    ///    tuning and spawn their bolts;
    /// 3. living actors and the player are pushed apart where they overlap;
    /// 4. projectiles fly; a hit on the player damages it; splash projectiles queue blasts;
    /// 5. queued blasts whose fuse ran out go off (see [`Combat::process_blasts`]);
    /// 6. corpses snap to their floor.
    pub fn tick(
        &mut self,
        map: &Map,
        defs: &Defs,
        player: &mut PlayerTarget,
        dt: f32,
    ) -> Vec<CombatEvent> {
        let mut out = Vec::new();
        for a in &mut self.actors {
            a.prev_pos = a.body.pos;
        }

        let perception = Perception {
            eye: player.eye,
            chest: player.body.pos + Vec3::Z * 0.6 * player.body.height,
            sector: player.body.sector,
            alive: player.vitals.health.alive(),
        };
        for i in 0..self.actors.len() {
            let a = &mut self.actors[i];
            if a.state == AiState::Dead {
                continue;
            }
            let def = defs.enemy(a.kind);
            let before = a.state;
            let (input, fire) = think(a, def, map, &perception, &mut self.rng, dt);
            out.extend(woke(i, before, a));
            if !a.alive() {
                continue;
            }
            let t = tuning(a, def);
            step_player(map, &mut a.body, &input, &t, dt);
            if let Some(dir) = fire {
                out.push(CombatEvent::ActorFired { actor: i });
                match def.attack {
                    EnemyAttack::Bolts { proj, .. } => self.spawn_bolt(map, i, dir, proj),
                    EnemyAttack::Hitscan { .. } => {
                        let mut bodies = vec![*player.body];
                        bodies.extend(self.actors.iter().map(|a| a.body));
                        let alive: Vec<bool> = self.actors.iter().map(Actor::alive).collect();
                        let v = volley(
                            map,
                            &self.actors[i],
                            i,
                            def,
                            dir,
                            &bodies,
                            |j| alive[j],
                            &mut self.rng,
                        );
                        out.extend(v.impacts.into_iter().map(|(point, normal, sector)| {
                            CombatEvent::Impact {
                                point,
                                normal,
                                sector,
                            }
                        }));
                        if v.player_damage > 0 {
                            let from = effective_muzzle(map, &self.actors[i]).0;
                            hurt_player(player, v.player_damage, from, &mut out);
                        }
                    }
                    EnemyAttack::Melee { damage, .. } => {
                        let a = &self.actors[i];
                        if swipe_reaches(a, def, player.body) {
                            hurt_player(player, damage, a.eye(), &mut out);
                        }
                    }
                    EnemyAttack::None => {}
                }
            }
        }

        self.separate(map, player.body);
        out.extend(self.fly(map, defs, player, dt));
        self.process_blasts(map, defs, player, dt, &mut out);

        for a in self.actors.iter_mut().filter(|a| !a.alive()) {
            a.body.pos.z = z_range(map, a.body.pos.truncate(), a.body.radius, a.body.sector).0;
        }
        out
    }

    /// Spawns actor `i`'s bolt at its muzzle, flying along `dir`.
    fn spawn_bolt(&mut self, map: &Map, i: usize, dir: Vec3, pd: ProjectileDef) {
        let a = &self.actors[i];
        let (muzzle, sector) = effective_muzzle(map, a);
        let id = self.take_id();
        self.projectiles.push(Projectile {
            id,
            pos: muzzle,
            prev: muzzle,
            vel: dir * pd.speed,
            sector,
            radius: pd.radius,
            damage: pd.damage,
            owner: Shooter::Actor(i),
            targets: Targets::Player,
            life: pd.life,
            gravity: pd.gravity,
            bounce: pd.bounce,
            remote: pd.remote,
            splash: pd.splash,
            resting: false,
        });
    }

    /// Pushes every living actor and the player apart by half their XY overlap each, when
    /// their height ranges overlap too. `clip_move` keeps both out of walls.
    fn separate(&mut self, map: &Map, player: &mut Body) {
        let step = Tuning::default().step_height;
        for a in self.actors.iter_mut().filter(|a| a.alive()) {
            let b = &mut a.body;
            let (pa, pp) = (b.pos.truncate(), player.pos.truncate());
            let dist = pa.distance(pp);
            let reach = b.radius + player.radius;
            let z_overlap =
                b.pos.z < player.pos.z + player.height && player.pos.z < b.pos.z + b.height;
            if dist >= reach || !z_overlap {
                continue;
            }
            let n = (pa - pp).try_normalize().unwrap_or(Vec2::X);
            if a.locomotion == Locomotion::Static {
                // Immovable: the player takes the whole push.
                clip_move(map, player, -n * (reach - dist), step);
                continue;
            }
            let push = n * (reach - dist) * 0.5;
            clip_move(map, b, push, step);
            clip_move(map, player, -push, step);
        }
    }

    /// Steps every projectile. A projectile that hits something or expires leaves play, and
    /// one with splash queues its blast (fuse 0, so it goes off this tick):
    /// - body hit: the direct `damage` to that body, then a blast at the impact point (whose
    ///   splash reaches that body too, once);
    /// - world hit: a blast `BLAST_NUDGE` off the surface, along its normal;
    /// - expiry: a blast where it is.
    ///
    /// A bouncing projectile (a pipe bomb) bounces off bodies as off walls, without damage. A
    /// resting one rides its sector's floor (lifts) and falls again if the floor drops away.
    fn fly(
        &mut self,
        map: &Map,
        defs: &Defs,
        player: &mut PlayerTarget,
        dt: f32,
    ) -> Vec<CombatEvent> {
        let mut out = Vec::new();
        let mut bodies = vec![*player.body];
        bodies.extend(self.actors.iter().map(|a| a.body));
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|p| {
            if p.resting {
                follow_floor(map, p);
            }
            let skip = |i: usize| i != BODY_PLAYER && !self.actors[i - 1].alive();
            match step_projectile(map, p, &bodies, skip, dt) {
                ProjectileStep::Flying | ProjectileStep::Resting => return true,
                ProjectileStep::HitBody(i) if p.bounce.is_some() => {
                    bounce_off_body(map, p, &bodies[i]);
                    return true;
                }
                ProjectileStep::Expired => {}
                ProjectileStep::HitWorld(h) => {
                    out.push(CombatEvent::Impact {
                        point: h.point,
                        normal: h.normal,
                        sector: h.sector,
                    });
                    // The blast centre: just off the surface, not inside it.
                    p.pos = h.point + h.normal * BLAST_NUDGE;
                    p.sector = sector_of(map, p.pos, h.sector);
                }
                ProjectileStep::HitBody(BODY_PLAYER) => {
                    hurt_player(player, p.damage, p.prev, &mut out);
                }
                ProjectileStep::HitBody(i) => {
                    out.extend(self.damage_actor(defs, i - 1, p.damage));
                }
            }
            if let Some(splash) = p.splash {
                // An expiry outside the map leaves `pos` stale; go off at the last good point.
                let (center, sector) = match map.find_sector(p.pos.truncate(), Some(p.sector)) {
                    Some(s) => (p.pos, s),
                    None => (p.prev, p.sector),
                };
                let blast = Blast {
                    center,
                    sector,
                    splash,
                    owner: p.owner,
                };
                self.queue_blast(0.0, blast);
            }
            out.push(CombatEvent::ProjectileGone(p.id));
            false
        });
        self.projectiles = projectiles;
        out
    }

    /// Actor indices and copies of the bodies of every living actor, for `Mechanics::tick`.
    /// Corpses are left out, so they never hold a door open or ride a lift.
    pub fn living_bodies(&self) -> (Vec<usize>, Vec<Body>) {
        self.actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.alive())
            .map(|(i, a)| (i, a.body))
            .unzip()
    }

    /// Copies bodies returned by `living_bodies` (after `Mechanics::tick`) back to their actors.
    pub fn write_back(&mut self, idx: &[usize], bodies: &[Body]) {
        for (&i, b) in idx.iter().zip(bodies) {
            self.actors[i].body = *b;
        }
    }
}

/// A resting projectile further than this above its floor (m) has lost it and falls again;
/// covers a lift lowering at up to ~5 m/s.
const REST_DROP: f32 = 0.08;
/// Gap left between a bouncing projectile and the body it bounced off (m).
const BODY_BOUNCE_NUDGE: f32 = 0.01;

/// Keeps a resting projectile on its sector's floor as that floor moves; one that the floor
/// dropped away from stops resting and falls.
fn follow_floor(map: &Map, p: &mut Projectile) {
    let floor = map.sectors[p.sector].floor_z + p.radius;
    if p.pos.z - floor > REST_DROP {
        p.resting = false;
    } else {
        p.pos.z = floor;
    }
}

/// Reflects a bouncing projectile off `body` (its top, its underside, or its side, which faces
/// horizontally away from the axis) with the projectile's restitution, and nudges it clear.
fn bounce_off_body(map: &Map, p: &mut Projectile, body: &Body) {
    let e = p.bounce.unwrap_or(0.0);
    let n = if p.pos.z >= body.pos.z + body.height {
        Vec3::Z
    } else if p.pos.z <= body.pos.z {
        Vec3::NEG_Z
    } else {
        (p.pos - body.pos)
            .truncate()
            .try_normalize()
            .unwrap_or_else(|| -p.vel.truncate().normalize_or(Vec2::X))
            .extend(0.0)
    };
    let into = p.vel.dot(n);
    if into < 0.0 {
        p.vel = (p.vel - 2.0 * into * n) * e;
    }
    let nudged = p.pos + n * BODY_BOUNCE_NUDGE;
    if let Some(s) = map.find_sector(nudged.truncate(), Some(p.sector)) {
        p.pos = nudged;
        p.sector = s;
    }
}

#[cfg(test)]
mod blast_tests;
#[cfg(test)]
mod tests;
