//! Combat orchestrator: one deterministic per-tick simulation of player attacks, enemy AI,
//! bolts and their effects. Plain Rust; the game turns the returned [`CombatEvent`]s into
//! sound, sparks and HUD feedback.
//!
//! Per fixed tick the game copies [`Combat::live_bombs`] into the arsenal, ticks it, calls
//! [`Combat::player_attack`] for each weapon event, then [`Combat::tick`], then hands
//! [`Combat::living_bodies`] (plus the players) to `Mechanics::tick` and copies them back with
//! [`Combat::write_back`]. Because player attacks run first, an actor killed by the player this
//! tick never fires.
//!
//! Split into [`player_attack`] (the player's weapon events), [`blasts`] (the explosion
//! queue) and [`specials`] (what the bosses' special moves do).

mod blasts;
mod player_attack;
mod specials;

pub use blasts::{BLAST_NOISE, BLAST_NUDGE, PendingBlast, Via};

use crate::actors::special::{self, Special};
use crate::actors::{
    Actor, AiState, GRUDGE_SECS, Perception, Target, choose_player, effective_muzzle, flyer_hover,
    hurt, retargets, swipe_reaches, think, tuning, turns_on_attacker, volley, wake,
};
use crate::collide::{Body, clip_move, z_range};
use crate::defs::{Defs, EnemyAttack, Locomotion, ProjectileDef};
use crate::destruct::{Destruct, hits_pane};
use crate::difficulty::Difficulty;
use crate::explosion::Blast;
use crate::hazard::{HazardClock, HazardKind};
use crate::health::DamageOutcome;
use crate::map::{Map, OPEN_GAP, SectorId, SwitchAction, WallId};
use crate::mechanics::Mechanics;
use crate::movement::{Tuning, step_flyer, step_player};
use crate::players::{BodyRef, MAX_PLAYERS, PlayerId, body_ref};
use crate::projectile::{Projectile, ProjectileStep, Shooter, Targets, step_projectile};
use crate::rng::Rng;
use crate::trace::{Hit, HitKind};
use crate::vitals::Vitals;
use blasts::sector_of;
use glam::{Vec2, Vec3};

/// What a combat tick did, for the game's presentation. The game's audio maps events to sounds
/// (`audio::cues`; a boss's `ActorWoke` and `PhaseChanged` get its roar), `ActorKilled` also
/// drives the kill stats and, for the player's own kills, the hero quips, and the game's combat
/// tick routes `DeathAction` through the level's mechanics. `BombsDetonated` and
/// `ProjectileGone` have no game reader: core tests consume them, and they are spare hooks.
#[derive(Debug, Clone, PartialEq)]
pub enum CombatEvent {
    /// A non-ignored hit on actor `actor`; `amount` is the total damage dealt this attack.
    ActorHurt {
        actor: usize,
        amount: i32,
    },
    /// Actor `actor` died. `by` is who dealt the killing damage; a barrel's blast belongs to
    /// whoever killed the barrel.
    ActorKilled {
        actor: usize,
        by: Shooter,
    },
    /// The actor left `Sleep` and is still alive.
    ActorWoke(usize),
    /// The actor spawned a bolt this tick.
    ActorFired {
        actor: usize,
    },
    /// `from` is the bolt's position at the start of the tick it hit, i.e. a point on its
    /// incoming line: `from - player` gives the direction the shot came from.
    PlayerHurt {
        player: PlayerId,
        amount: i32,
        from: Vec3,
    },
    /// Reported exactly once, on the hit that empties the player's health.
    PlayerKilled(PlayerId),
    /// A blast opened crack wall `s`; its mesh changes as the mover animates.
    CrackOpened(SectorId),
    /// The fixture of light `i` (index in `Map::lights`) broke; the light is out for good.
    LightBroken(usize),
    /// The player entered a secret sector for the first time.
    SecretFound,
    /// The player's floor burned them; usually followed by a `PlayerHurt` (none when the hurt is
    /// ignored, e.g. an invulnerable demo run).
    HazardBurn {
        player: PlayerId,
        kind: HazardKind,
    },
    /// A shot or projectile hit the world at `point` (in `sector`).
    Impact {
        point: Vec3,
        normal: Vec3,
        sector: SectorId,
        /// The wall hit (`HitKind::Wall`); `None` for a floor or ceiling.
        wall: Option<WallId>,
    },
    ProjectileGone(u32),
    /// A blast went off (rocket, bomb, barrel or a scripted level blast).
    Explosion {
        point: Vec3,
        radius: f32,
    },
    /// The player set off their live pipe bombs (once per detonation).
    BombsDetonated,
    /// A shot, projectile, kick or blast shattered the glass pane in portal `wall`. `dirty`
    /// lists the sectors to re-mesh.
    GlassBroken {
        /// The side that was hit (owned by the shooter's sector); for a blast, the lower id.
        wall: WallId,
        dirty: Vec<SectorId>,
    },
    /// Boss `actor` crossed a health threshold into `phase` (`EnemyDef::phase_for`).
    PhaseChanged {
        actor: usize,
        phase: u8,
    },
    /// An actor's `on_death` on the tick it reached `Dead`; the game fires the channel or ends
    /// the level.
    DeathAction(SwitchAction),
    /// Actor `actor` started digging down (a burrow, `actors::special`).
    Burrowed {
        actor: usize,
    },
    /// Burrowed actor `actor` came up again (where it now stands).
    Surfaced {
        actor: usize,
    },
    /// A new actor joined play at the end of `Combat::actors` (a burrow's minion); it is
    /// awake. The game gives it a visual.
    ActorSpawned(usize),
    /// Actor `actor` slammed the floor at `point`: bodies on the ground within `radius` took
    /// damage. The game shakes the screen.
    GroundPound {
        actor: usize,
        point: Vec3,
        radius: f32,
    },
}

impl CombatEvent {
    /// The `Impact` of world hit `h`, carrying the wall when it is one.
    pub fn impact(h: &Hit) -> Self {
        CombatEvent::Impact {
            point: h.point,
            normal: h.normal,
            sector: h.sector,
            wall: match h.kind {
                HitKind::Wall(w) => Some(w),
                _ => None,
            },
        }
    }
}

/// One player as combat sees it this tick; `tick` takes them all, `players[k]` being
/// `PlayerId(k)`.
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
    /// What shots and blasts have broken (glass so far).
    pub destruct: Destruct,
    /// Multiplier on damage the player takes; the difficulty sets it. 1.0 at spawn.
    pub damage_scale: f32,
    /// Each player's hazard-floor clock (index = `PlayerId`), grown on demand by `tick`; reset
    /// with the level.
    pub hazard: Vec<HazardClock>,
    /// The skill played; it sets the cap on live minions. `Normal` at spawn.
    pub skill: Difficulty,
    next_id: u32,
}

/// A stable 64-bit seed from a level name (FNV-1a; does not depend on the Rust version).
pub fn level_seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Damages player `id` and reports `PlayerHurt { from }` (and `PlayerKilled` on the fatal hit).
fn hurt_player(
    player: &mut PlayerTarget,
    id: PlayerId,
    amount: i32,
    scale: f32,
    from: Vec3,
    out: &mut Vec<CombatEvent>,
) {
    let amount = ((amount as f32) * scale).round().max(1.0) as i32;
    match player.vitals.damage(amount) {
        DamageOutcome::Ignored => {}
        outcome => {
            out.push(CombatEvent::PlayerHurt {
                player: id,
                amount,
                from,
            });
            if outcome == DamageOutcome::Killed {
                out.push(CombatEvent::PlayerKilled(id));
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
            destruct: Destruct::new(map),
            damage_scale: 1.0,
            hazard: Vec::new(),
            skill: Difficulty::Normal,
            next_id: 0,
        }
    }

    /// Player `id` leaves; every player `j > id` becomes `j - 1`. Their projectiles (bombs
    /// included) vanish, their pending blasts become the level's, and actors that hunted them
    /// go back to player 0 with no grudge. The caller drops the player's body and vitals.
    pub fn remove_player(&mut self, id: PlayerId) {
        let gone = |s: Shooter| s == Shooter::Player(id);
        let shift = |s: Shooter| match s {
            Shooter::Player(PlayerId(j)) if j > id.0 => Shooter::Player(PlayerId(j - 1)),
            s => s,
        };
        self.projectiles.retain(|p| !gone(p.owner));
        for p in &mut self.projectiles {
            p.owner = shift(p.owner);
        }
        for pb in &mut self.pending_blasts {
            pb.blast.owner = if gone(pb.blast.owner) {
                Shooter::World
            } else {
                shift(pb.blast.owner)
            };
        }
        for a in &mut self.actors {
            if let Target::Player(PlayerId(j)) = a.target {
                if j == id.0 {
                    a.target = Target::Player(PlayerId(0));
                    a.grudge = 0.0;
                } else if j > id.0 {
                    a.target = Target::Player(PlayerId(j - 1));
                }
            }
        }
        debug_assert!(usize::from(id.0) < MAX_PLAYERS, "no such player");
        if (id.0 as usize) < self.hazard.len() {
            self.hazard.remove(id.0 as usize);
        }
    }

    /// Breaks the glass in wall `w`, reporting `GlassBroken` if there was any.
    fn break_glass(&mut self, map: &mut Map, w: WallId, out: &mut Vec<CombatEvent>) {
        let dirty = self.destruct.break_glass(map, w);
        if !dirty.is_empty() {
            out.push(CombatEvent::GlassBroken { wall: w, dirty });
        }
    }

    /// A fresh projectile id.
    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Applies `n` damage from `by` to actor `i` and reports what happened. An actor with a
    /// `death_splash` (a barrel) that dies queues its own blast, owned by its killer `by`, on its
    /// `death_fuse`; `Health` reports the kill only once, so it is queued only once. An actor
    /// hurt by another one may turn on it (`retargets`), and one hunting a player turns on
    /// another player who hurts it (`turns_on_attacker`). A boss ignores its own damage (its
    /// rocket splash) and may change phase on a hit (`PhaseChanged`).
    fn damage_actor(&mut self, defs: &Defs, i: usize, n: i32, by: Shooter) -> Vec<CombatEvent> {
        self.damage_actor_via(defs, i, n, by, Via::Direct)
    }

    /// `damage_actor` with the route the damage took (`via`): a boss shrugs off its own direct
    /// splash, but a barrel it set off still hurts it. Kill credit and retargeting use `by`.
    fn damage_actor_via(
        &mut self,
        defs: &Defs,
        i: usize,
        n: i32,
        by: Shooter,
        via: Via,
    ) -> Vec<CombatEvent> {
        let def = defs.enemy(self.actors[i].kind);
        // A boss shrugs off its own rocket splash.
        if def.boss && by == Shooter::Actor(i) && via == Via::Direct {
            return Vec::new();
        }
        let a = &mut self.actors[i];
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
                out.push(CombatEvent::ActorKilled { actor: i, by });
            }
        }
        out.extend(woke(i, before, a));
        if outcome == DamageOutcome::Hurt {
            let phase = def.phase_for(a.health.hp as f32 / a.health.max as f32);
            if phase != a.phase {
                let phased = def.phased(phase);
                a.phase = phase;
                a.muzzle_offset = Vec3::from(phased.muzzle);
                // A phase's special moves come into reach after their `first` delay.
                a.burrow_in = phased.burrow.map_or(0.0, |b| b.first);
                a.pound_in = phased.pound.map_or(0.0, |p| p.first);
                out.push(CombatEvent::PhaseChanged { actor: i, phase });
            }
        }
        if outcome != DamageOutcome::Ignored
            && let Shooter::Actor(k) = by
            && k != i
            && retargets(&self.actors[i], &self.actors[k])
        {
            self.actors[i].target = Target::Actor(k);
        }
        // Shot by another player than the one it hunts: it turns on the shooter. Either way, a
        // player it now hunts and who hurt it keeps it for `GRUDGE_SECS` (see `tick`).
        if outcome != DamageOutcome::Ignored
            && let Shooter::Player(p) = by
            && matches!(self.actors[i].target, Target::Player(_))
            && turns_on_attacker(&self.actors[i])
        {
            let a = &mut self.actors[i];
            a.target = Target::Player(p);
            a.grudge = GRUDGE_SECS;
        }
        if outcome == DamageOutcome::Killed
            && let Some(splash) = def.death_splash
        {
            let blast = Blast {
                center: middle,
                sector: self.actors[i].body.sector,
                splash,
                owner: by,
            };
            self.queue_barrel_blast(def.death_fuse, blast);
        }
        out
    }

    /// A noise of loudness `radius` (metres) at `at` in `sector`. Sound spreads through
    /// portals whose live opening is taller than 0.1 m (so closed doors block it) and that hold
    /// no intact glass, and wakes every sleeping actor in a reached sector within `radius` of
    /// `at`. Returns the woken actors in ascending order.
    pub fn make_noise(&mut self, map: &Map, at: Vec3, sector: SectorId, radius: f32) -> Vec<usize> {
        let mut heard = vec![false; map.sectors.len()];
        heard[sector] = true;
        let mut queue = std::collections::VecDeque::from([sector]);
        while let Some(s) = queue.pop_front() {
            let here = &map.sectors[s];
            for n in map.passages(s) {
                let there = &map.sectors[n];
                let gap = here.ceil_z.min(there.ceil_z) - here.floor_z.max(there.floor_z);
                if !heard[n] && gap > OPEN_GAP {
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

    /// One fixed tick for every player in `players` (`players[k]` is `PlayerId(k)`; at least
    /// one, at most `MAX_PLAYERS`), in this (load-bearing) order:
    /// 1. per player in id order: a secret sector entered for the first time reports
    ///    `SecretFound`, and its hazard floor may burn it (`hazard`, one clock per player);
    /// 2. remember every actor's `prev_pos`;
    /// 3. AI: every actor hunting a player picks which one (`choose_player`, only with more than
    ///    one player), then runs its special moves (`actors::special`: burrows and pounds, whose
    ///    minions and shockwaves resolve here) or else thinks (dying ones count down); living
    ///    ones move with the enemy tuning and spawn their bolts. Minions join at the end of
    ///    `actors` and first act next tick;
    /// 4. living actors and the players are pushed apart where they overlap, then the living
    ///    players from each other (with partners, a dead player takes no part);
    /// 5. projectiles fly; a hit on a player damages it; splash projectiles queue blasts (with
    ///    partners, enemy shots pass a dead player);
    /// 6. scripted blasts the level's channels set off join the queue (on their `delay`), and
    ///    queued blasts whose fuse ran out go off (see [`Combat::process_blasts`]);
    /// 7. corpses snap to their floor (dead flyers fall to it first).
    ///
    /// Shots, projectiles and blasts that shatter glass change `map` (through `destruct`) and
    /// report `GlassBroken`. `mech` is where blasts open crack walls (`CrackOpened`; the mover
    /// then animates through `Mechanics::tick`); glass does not need it.
    pub fn tick(
        &mut self,
        map: &mut Map,
        mech: &mut Mechanics,
        defs: &Defs,
        players: &mut [PlayerTarget],
        dt: f32,
    ) -> Vec<CombatEvent> {
        debug_assert!(
            (1..=MAX_PLAYERS).contains(&players.len()),
            "{} players",
            players.len()
        );
        let mut out = Vec::new();
        if self.hazard.len() < players.len() {
            self.hazard.resize_with(players.len(), HazardClock::default);
        }
        for (k, player) in players.iter_mut().enumerate() {
            let id = PlayerId(k as u8);
            if self.destruct.enter_sector(map, player.body.sector) {
                out.push(CombatEvent::SecretFound);
            }
            let touching = crate::hazard::contact(map, player.body);
            if let Some(h) = self.hazard[k].tick(touching, dt)
                && player.vitals.health.alive()
            {
                out.push(CombatEvent::HazardBurn {
                    player: id,
                    kind: h.kind,
                });
                let at = player.body.pos;
                hurt_player(player, id, h.damage, self.damage_scale, at, &mut out);
            }
        }
        for a in &mut self.actors {
            a.prev_pos = a.body.pos;
        }

        let views: Vec<Perception> = players
            .iter()
            .map(|p| Perception {
                eye: p.eye,
                chest: p.body.pos + Vec3::Z * 0.6 * p.body.height,
                sector: p.body.sector,
                alive: p.vitals.health.alive(),
            })
            .collect();
        let n = players.len();
        // Bodies as they stood at the start of the tick, so no actor sees another mid-update; a
        // player-only fight reads exactly what it read before (same rng draws).
        let snapshot: Vec<(Body, bool)> = self
            .actors
            .iter()
            .map(|a| (a.body, a.targetable()))
            .collect();
        for i in 0..self.actors.len() {
            let a = &mut self.actors[i];
            if a.state == AiState::Dead {
                continue;
            }
            if let Target::Actor(j) = a.target
                && !snapshot[j].1
            {
                a.target = Target::default();
            }
            a.grudge = (a.grudge - dt).max(0.0);
            if let Target::Player(current) = a.target {
                let held = a.grudge > 0.0 && views.get(current.0 as usize).is_some_and(|v| v.alive);
                // Alone, player 0 is the only choice (and nothing is traced). A grudge holds a
                // living attacker without a new choice; a static actor never hunts, so it keeps
                // whom it has.
                let fixed = a.locomotion == Locomotion::Static && (current.0 as usize) < n;
                let id = if n <= 1 {
                    PlayerId(0)
                } else if held || fixed {
                    current
                } else {
                    let def = defs.enemy(a.kind).phased(a.phase);
                    choose_player(map, a, &def, current, &views)
                };
                a.target = Target::Player(id);
            }
            let perception = match a.target {
                // Always below `n`: just picked.
                Target::Player(id) => views[id.0 as usize],
                Target::Actor(j) => Perception::of_body(&snapshot[j].0),
            };
            let def = defs.enemy(a.kind).phased(a.phase);
            let def = &*def;
            let before = a.state;
            let (input, fire, special_move) =
                match special::step(a, def, map, &perception, &mut self.rng, dt) {
                    Special::Think => {
                        let (input, fire) = think(a, def, map, &perception, &mut self.rng, dt);
                        (input, fire, None)
                    }
                    Special::Busy(m) => (Default::default(), None, m),
                };
            out.extend(woke(i, before, a));
            if a.state == AiState::Dead
                && let Some(action) = a.on_death.take()
            {
                out.push(CombatEvent::DeathAction(action));
            }
            if !a.alive() {
                continue;
            }
            if let Some(hover) = {
                let floor = z_range(map, a.body.pos.truncate(), a.body.radius, a.body.sector).0;
                flyer_hover(a, &perception, floor)
            } {
                let wish = input.wish.extend(0.0);
                step_flyer(map, &mut a.body, wish, def.speed, hover, dt);
            } else {
                let t = tuning(a, def);
                step_player(map, &mut a.body, &input, &t, dt);
            }
            if let Some(m) = special_move {
                self.special_move(map, defs, i, m, players, &mut out);
            }
            if let Some(dir) = fire {
                out.push(CombatEvent::ActorFired { actor: i });
                let target = self.actors[i].target;
                match def.attack {
                    EnemyAttack::Bolts { proj, .. } => self.spawn_bolt(map, i, dir, proj),
                    EnemyAttack::Hitscan { .. } => {
                        let mut bodies: Vec<Body> = players.iter().map(|p| *p.body).collect();
                        bodies.extend(self.actors.iter().map(|a| a.body));
                        let alive: Vec<bool> = self.actors.iter().map(Actor::targetable).collect();
                        // With partners, a dead player's body lets pellets pass.
                        let living =
                            |id: PlayerId| n <= 1 || players[id.0 as usize].vitals.health.alive();
                        let v = volley(
                            map,
                            &self.actors[i],
                            i,
                            def,
                            dir,
                            &bodies,
                            n,
                            |r| match r {
                                BodyRef::Player(id) => living(id),
                                BodyRef::Actor(j) => alive[j],
                            },
                            &mut self.rng,
                        );
                        out.extend(v.impacts.iter().map(CombatEvent::impact));
                        for w in v.glass {
                            self.break_glass(map, w, &mut out);
                        }
                        for (j, d) in v.actor_damage {
                            out.extend(self.damage_actor(defs, j, d, Shooter::Actor(i)));
                        }
                        if !v.player_damage.is_empty() {
                            let from = effective_muzzle(map, &self.actors[i]).0;
                            for (id, d) in v.player_damage {
                                let p = &mut players[id.0 as usize];
                                hurt_player(p, id, d, self.damage_scale, from, &mut out);
                            }
                        }
                    }
                    EnemyAttack::Melee { damage, .. } => match target {
                        Target::Player(id) => {
                            let a = &self.actors[i];
                            let p = &mut players[id.0 as usize];
                            if swipe_reaches(a, def, p.body) {
                                hurt_player(p, id, damage, self.damage_scale, a.eye(), &mut out);
                            }
                        }
                        Target::Actor(j) => {
                            if swipe_reaches(&self.actors[i], def, &self.actors[j].body) {
                                out.extend(self.damage_actor(defs, j, damage, Shooter::Actor(i)));
                            }
                        }
                    },
                    EnemyAttack::None => {}
                }
            }
        }

        self.separate(map, players);
        out.extend(self.fly(map, defs, players, dt));
        self.queue_scripted(map, mech);
        self.process_blasts(map, mech, defs, players, dt, &mut out);

        let gravity = Tuning::default().gravity;
        for a in self.actors.iter_mut().filter(|a| !a.alive()) {
            let floor = z_range(map, a.body.pos.truncate(), a.body.radius, a.body.sector).0;
            if matches!(a.locomotion, Locomotion::Fly { .. }) && a.body.pos.z > floor {
                // A dead flyer drops with normal gravity, then lies on the floor.
                a.body.vel.z -= gravity * dt;
                a.body.pos.z = (a.body.pos.z + a.body.vel.z * dt).max(floor);
            } else {
                a.body.pos.z = floor;
            }
            if a.body.pos.z == floor {
                a.body.vel = Vec3::ZERO;
            }
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
            // Enemy bolts hit whoever is in the way; the owner is excluded by
            // `Projectile::ignores`.
            targets: Targets::All,
            life: pd.life,
            gravity: pd.gravity,
            bounce: pd.bounce,
            remote: pd.remote,
            splash: pd.splash,
            resting: false,
        });
    }

    /// Pushes every living actor and each player apart by half their XY overlap each, when
    /// their height ranges overlap too (players in id order; a static actor pushes the player
    /// the whole way; with more than one player, dead ones take no part). Then each pair of
    /// living players, once, likewise. `clip_move` keeps them all out of walls.
    fn separate(&mut self, map: &Map, players: &mut [PlayerTarget]) {
        let step = Tuning::default().step_height;
        let n = players.len();
        for player in players.iter_mut() {
            if n > 1 && !player.vitals.health.alive() {
                continue;
            }
            let player = &mut *player.body;
            for a in self.actors.iter_mut().filter(|a| a.targetable()) {
                let b = &mut a.body;
                let Some((dir, gap)) = overlap(b, player) else {
                    continue;
                };
                if a.locomotion == Locomotion::Static {
                    // Immovable: the player takes the whole push.
                    clip_move(map, player, -dir * gap, step);
                    continue;
                }
                let push = dir * gap * 0.5;
                clip_move(map, b, push, step);
                clip_move(map, player, -push, step);
            }
        }
        for j in 1..players.len() {
            let (head, tail) = players.split_at_mut(j);
            let q = &mut tail[0];
            for p in head.iter_mut() {
                if !p.vitals.health.alive() || !q.vitals.health.alive() {
                    continue;
                }
                let Some((n, gap)) = overlap(q.body, p.body) else {
                    continue;
                };
                let push = n * gap * 0.5;
                clip_move(map, q.body, push, step);
                clip_move(map, p.body, -push, step);
            }
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
        map: &mut Map,
        defs: &Defs,
        players: &mut [PlayerTarget],
        dt: f32,
    ) -> Vec<CombatEvent> {
        let mut out = Vec::new();
        let n = players.len();
        let mut bodies: Vec<Body> = players.iter().map(|p| *p.body).collect();
        bodies.extend(self.actors.iter().map(|a| a.body));
        // With partners, projectiles pass a player who was dead when they set off this tick.
        let dead: Vec<bool> = players
            .iter()
            .map(|p| n > 1 && !p.vitals.health.alive())
            .collect();
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|p| {
            if p.resting {
                follow_floor(map, p);
            }
            let skip = |i: usize| match body_ref(n, i) {
                BodyRef::Player(id) => dead[id.0 as usize],
                BodyRef::Actor(a) => !self.actors[a].targetable(),
            };
            match step_projectile(map, p, &bodies, n, skip, dt) {
                ProjectileStep::Flying | ProjectileStep::Resting => return true,
                ProjectileStep::HitBody(i) if p.bounce.is_some() => {
                    bounce_off_body(map, p, &bodies[i]);
                    return true;
                }
                ProjectileStep::Expired => {}
                ProjectileStep::HitWorld(h) => {
                    match h.kind {
                        // The projectile is spent on the pane; a rocket still goes off.
                        HitKind::Wall(w) if hits_pane(map, &h) => {
                            self.break_glass(map, w, &mut out);
                        }
                        _ => out.push(CombatEvent::impact(&h)),
                    }
                    // The blast centre: just off the surface, not inside it.
                    p.pos = h.point + h.normal * BLAST_NUDGE;
                    p.sector = sector_of(map, p.pos, h.sector);
                }
                ProjectileStep::HitBody(i) => match body_ref(n, i) {
                    BodyRef::Player(id) => {
                        let player = &mut players[id.0 as usize];
                        hurt_player(player, id, p.damage, self.damage_scale, p.prev, &mut out);
                    }
                    BodyRef::Actor(a) => {
                        out.extend(self.damage_actor(defs, a, p.damage, p.owner));
                    }
                },
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

    /// Health fraction of the first awake, living boss: drives the HUD boss bar.
    pub fn boss_health(&self) -> Option<f32> {
        self.actors
            .iter()
            .find(|a| a.boss && a.alive() && a.state != AiState::Sleep)
            .map(|a| a.health.hp as f32 / a.health.max as f32)
    }

    /// Actor indices and copies of the bodies of every living actor, for `Mechanics::tick`.
    /// Corpses and burrowed actors are left out, so they never hold a door open or ride a lift.
    pub fn living_bodies(&self) -> (Vec<usize>, Vec<Body>) {
        self.actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.targetable())
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

/// How far apart (unit XY direction from `other` to `b`, and the depth) `b` and `other` must be
/// pushed: `None` unless they overlap in XY and in height.
fn overlap(b: &Body, other: &Body) -> Option<(Vec2, f32)> {
    let (pb, po) = (b.pos.truncate(), other.pos.truncate());
    let dist = pb.distance(po);
    let reach = b.radius + other.radius;
    let z_overlap = b.pos.z < other.pos.z + other.height && other.pos.z < b.pos.z + b.height;
    if dist >= reach || !z_overlap {
        return None;
    }
    Some(((pb - po).try_normalize().unwrap_or(Vec2::X), reach - dist))
}

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
mod boss_tests;
#[cfg(test)]
mod coop_tests;
#[cfg(test)]
mod crack_tests;
#[cfg(test)]
mod drone_tests;
#[cfg(test)]
mod glass_tests;
#[cfg(test)]
mod hazard_tests;
#[cfg(test)]
mod infight_tests;
#[cfg(test)]
mod light_tests;
#[cfg(test)]
mod queen_tests;
#[cfg(test)]
mod scripted_tests;
#[cfg(test)]
mod tests;
