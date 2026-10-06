//! Enemy actors: the Grunt's per-tick brain (sleep → alert → chase ⇄ attack, pain, death),
//! sector-graph chasing and damage reactions.
//!
//! `think` only decides; the caller moves the body with `movement::step_player` using
//! `def.tuning()` and spawns a bolt for the returned fire direction.

use crate::collide::Body;
use crate::defs::{EnemyAttack, EnemyDef, Locomotion};
use crate::health::{DamageOutcome, Health};
use crate::map::{ActorKind, ActorSpawn, Map, SectorId, WallId};
use crate::movement::{FLYER_MIN_CLEARANCE, MoveInput, Pass, Tuning};
use crate::rng::Rng;
use crate::trace::{Hit, HitKind, Ray, trace};
use glam::{Vec2, Vec3};

pub mod steer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiState {
    Sleep,
    /// Awake; `t` counts up to the reaction time, then the actor chases.
    Alert {
        t: f32,
    },
    Chase,
    /// Firing a burst: `t` is the time until the next shot, `left` the shots still to fire.
    Attack {
        t: f32,
        left: u32,
    },
    /// Frozen; `t` counts down.
    Pain {
        t: f32,
    },
    /// Collapsing; `t` counts down, then `Dead`.
    Dying {
        t: f32,
    },
    Dead,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub kind: ActorKind,
    pub body: Body,
    pub prev_pos: Vec3,
    /// Heading in radians (0 = +x, counter-clockwise).
    pub angle: f32,
    pub health: Health,
    pub state: AiState,
    /// Time until the next burst may start.
    pub refire: f32,
    /// Time until the chase route (`hop`) is recomputed.
    pub repath: f32,
    /// Next sector on the route to the player, when chasing out of sight.
    pub hop: Option<SectorId>,
    /// Corner of the current sector the actor is steering around, when the line to its goal is
    /// blocked (see `steer`).
    pub corner: Option<Vec2>,
    /// Seconds until pain may be rolled again; set to `pain_cooldown` when Pain ends.
    pub pain_ready_in: f32,
    /// Strafe timer: the sign is the sidestep direction (+ = left of the facing, − = right), the
    /// magnitude the seconds until it flips. 0 means "not drawn yet".
    pub strafe: f32,
    /// Muzzle offset (forward, side, up) from the feet; see `EnemyDef::muzzle`.
    pub muzzle_offset: Vec3,
    /// Copied from the def; `Static` actors never think, wake or feel pain.
    pub locomotion: Locomotion,
    /// Whom it hunts: the player at spawn, another actor after infighting.
    pub target: Target,
}

/// Whom an actor hunts. Infighting points it at another actor (by index; actors are never
/// removed, so indices stay valid); when that one dies it goes back to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    #[default]
    Player,
    Actor(usize),
}

/// What an actor knows about its target (the player, or another actor) this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perception {
    /// The target's eye point (what actors try to see).
    pub eye: Vec3,
    /// The target's chest point (feet + 0.6 · height), where actors aim.
    pub chest: Vec3,
    pub sector: SectorId,
    pub alive: bool,
}

impl Perception {
    /// Another actor's body as a target: eye at 0.9 and chest at 0.6 of its height.
    pub fn of_body(b: &Body) -> Perception {
        Perception {
            eye: b.pos + Vec3::Z * 0.9 * b.height,
            chest: b.pos + Vec3::Z * 0.6 * b.height,
            sector: b.sector,
            alive: true,
        }
    }
}

/// Doom-style infighting: whether `victim`, just hurt by `attacker`, turns on it. Both alive,
/// different kinds, neither static (barrels never hunt nor are hunted).
pub fn retargets(victim: &Actor, attacker: &Actor) -> bool {
    victim.alive()
        && attacker.alive()
        && victim.kind != attacker.kind
        && victim.locomotion != Locomotion::Static
        && attacker.locomotion != Locomotion::Static
}

/// How long a chase route stays valid before `next_hop` runs again.
pub const REPATH: f32 = 0.5;
/// Sidestep strength while strafing, as a fraction of the actor's speed.
pub const STRAFE: f32 = 0.6;
/// Range of the strafe flip interval, in seconds.
pub const STRAFE_FLIP: (f32, f32) = (0.8, 1.5);
/// Body height of a dying or dead actor.
pub const CORPSE_HEIGHT: f32 = 0.3;
/// Chasing actors with the player in sight stop closing in at this distance.
pub const CLOSE_ENOUGH: f32 = 1.5;
/// How far past a portal's midpoint the route waypoint lies, so the body actually crosses.
const PORTAL_OVERSHOOT: f32 = 0.25;
/// A steering corner counts as reached within this distance.
const CORNER_REACHED: f32 = 0.15;
/// A new corner must beat the current one by this much (m) to replace it on a repath.
const CORNER_STICKY: f32 = 0.75;

impl Actor {
    /// A fresh actor at a map spawn point, or `None` if the spawn lies outside every sector.
    /// Sleeping spawns start in `Sleep`, awake ones in `Alert` (they still react before chasing).
    pub fn new(map: &Map, def: &EnemyDef, spawn: &ActorSpawn) -> Option<Actor> {
        let mut body = Body::spawn(map, spawn.pos, def.radius, def.height)?;
        if let Locomotion::Fly { hover, .. } = def.locomotion {
            // Flyers start at their cruising height, not on the floor.
            let ceil = map.sectors[body.sector].ceil_z;
            let floor = body.pos.z;
            body.pos.z = (floor + hover)
                .max(floor + FLYER_MIN_CLEARANCE)
                .min(ceil - def.height)
                .max(floor);
            body.on_ground = false;
        }
        Some(Actor {
            kind: spawn.kind,
            body,
            prev_pos: body.pos,
            angle: spawn.angle,
            health: Health::new(def.health),
            state: if spawn.asleep || def.locomotion == Locomotion::Static {
                AiState::Sleep
            } else {
                AiState::Alert { t: 0.0 }
            },
            refire: 0.0,
            repath: 0.0,
            hop: None,
            corner: None,
            pain_ready_in: 0.0,
            strafe: 0.0,
            muzzle_offset: Vec3::from(def.muzzle),
            locomotion: def.locomotion,
            target: Target::Player,
        })
    }

    /// Eye point: feet + 0.9 · height.
    pub fn eye(&self) -> Vec3 {
        self.body.pos + Vec3::Z * 0.9 * self.body.height
    }

    /// Muzzle point: the def's (forward, side, up) offset from the feet, rotated by the
    /// heading (side is positive to the left). Bolts and pellets start here.
    pub fn muzzle(&self) -> Vec3 {
        let (sin, cos) = self.angle.sin_cos();
        let (fwd, side, up) = self.muzzle_offset.into();
        self.body.pos + Vec3::new(fwd * cos - side * sin, fwd * sin + side * cos, up)
    }

    /// Dying and dead actors are no longer targets.
    pub fn alive(&self) -> bool {
        !matches!(self.state, AiState::Dying { .. } | AiState::Dead)
    }
}

/// Noise reached this actor: a sleeper becomes `Alert { t: 0 }`; any other state is unchanged.
pub fn wake(a: &mut Actor) {
    if a.state == AiState::Sleep && a.locomotion != Locomotion::Static {
        a.state = AiState::Alert { t: 0.0 };
    }
}

/// Heading of a horizontal direction (0 = +x, counter-clockwise).
fn heading(d: Vec2) -> f32 {
    d.y.atan2(d.x)
}

/// Absolute difference between two headings, in [0, π].
fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

/// Seconds a melee actor lunges before it swipes.
pub const LUNGE_TIME: f32 = 0.2;

/// Seconds a swooping flyer dives before its first shot.
pub const SWOOP_WINDUP: f32 = 0.3;
/// How far below the player's eye a swooping flyer dives (m).
const SWOOP_BELOW_EYE: f32 = 0.3;

/// The cruising height a flying actor steers to this tick, in metres above `floor` (the floor
/// under it): its `hover`, except that a swooper in `Attack` dives to just below the player's
/// eye. `None` for walkers.
pub fn flyer_hover(a: &Actor, p: &Perception, floor: f32) -> Option<f32> {
    let Locomotion::Fly { hover, swoop } = a.locomotion else {
        return None;
    };
    Some(
        if swoop && matches!(a.state, AiState::Attack { .. }) && p.alive {
            p.eye.z - SWOOP_BELOW_EYE - floor
        } else {
            hover
        },
    )
}

/// Movement tuning for this tick: the def's, except that a melee actor in its lunge moves at
/// `lunge_speed`.
pub fn tuning(a: &Actor, def: &EnemyDef) -> Tuning {
    match (a.state, def.attack) {
        (AiState::Attack { .. }, EnemyAttack::Melee { lunge_speed, .. }) => Tuning {
            max_speed: lunge_speed,
            ..def.tuning()
        },
        _ => def.tuning(),
    }
}

/// One tick of AI. Returns the movement wish for `step_player` and, when the actor attacks, the
/// unit direction from `a.muzzle()` to the player's chest (jittered by the aim error). The
/// caller resolves the attack by `def.attack`: it spawns a bolt, traces a volley (`volley`) or
/// swipes (`swipe_reaches`).
///
/// Dying and dead actors (and every actor while the player is dead) return a zero input and
/// never fire. A dead player wakes nobody and freezes every state but `Dying`.
pub fn think(
    a: &mut Actor,
    def: &EnemyDef,
    map: &Map,
    p: &Perception,
    rng: &mut Rng,
    dt: f32,
) -> (MoveInput, Option<Vec3>) {
    let idle = (MoveInput::default(), None);
    match a.state {
        AiState::Dead => return idle,
        AiState::Dying { t } => {
            a.state = if t - dt <= 0.0 {
                AiState::Dead
            } else {
                AiState::Dying { t: t - dt }
            };
            return idle;
        }
        _ => {}
    }
    if a.locomotion == Locomotion::Static || !p.alive {
        return idle;
    }
    a.refire = (a.refire - dt).max(0.0);
    if !matches!(a.state, AiState::Pain { .. }) {
        a.pain_ready_in = (a.pain_ready_in - dt).max(0.0);
    }

    let eye = a.eye();
    let to_player = p.eye - eye;
    let flat = to_player.truncate();
    let dist = to_player.length();
    let sees = dist <= def.sight_range && crate::trace::can_see(map, eye, a.body.sector, p.eye);

    match a.state {
        AiState::Sleep => {
            let in_fov = angle_diff(a.angle, heading(flat)) <= (def.fov_deg / 2.0).to_radians();
            if in_fov && sees {
                a.state = AiState::Alert { t: 0.0 };
            }
            idle
        }
        AiState::Alert { t } => {
            if sees {
                a.angle = heading(flat);
            }
            let t = t + dt;
            a.state = if t >= def.reaction {
                AiState::Chase
            } else {
                AiState::Alert { t }
            };
            idle
        }
        AiState::Pain { t } => {
            a.state = if t - dt <= 0.0 {
                a.pain_ready_in = def.pain_cooldown;
                AiState::Chase
            } else {
                AiState::Pain { t: t - dt }
            };
            idle
        }
        AiState::Attack { t, left } => {
            if !sees {
                // Lost the target mid-burst: give up the rest of it.
                a.refire = def.attack_refire;
                a.state = AiState::Chase;
                return idle;
            }
            a.angle = heading(flat);
            let t = t - dt;
            if t > 0.0 {
                a.state = AiState::Attack { t, left };
                return (lunge(def, flat), None);
            }
            if left == 0 {
                a.refire = def.attack_refire;
                a.state = AiState::Chase;
                return idle;
            }
            (MoveInput::default(), Some(shoot(a, def, p, rng, left)))
        }
        AiState::Chase => {
            let armed = def.attack != EnemyAttack::None;
            if sees && dist <= def.attack_range && a.refire <= 0.0 && armed {
                a.angle = heading(flat);
                if matches!(def.attack, EnemyAttack::Melee { .. }) {
                    a.state = AiState::Attack {
                        t: LUNGE_TIME,
                        left: 1,
                    };
                    return (lunge(def, flat), None);
                }
                let burst = def.attack.burst();
                if matches!(a.locomotion, Locomotion::Fly { swoop: true, .. }) {
                    // A swooper dives first and fires when it is down (see `flyer_hover`).
                    a.state = AiState::Attack {
                        t: SWOOP_WINDUP,
                        left: burst,
                    };
                    return (MoveInput::default(), None);
                }
                return (MoveInput::default(), Some(shoot(a, def, p, rng, burst)));
            }
            (chase(a, def, map, p, sees, rng, dt), None)
        }
        AiState::Dying { .. } | AiState::Dead => idle,
    }
}

/// Full-speed wish along `flat` while lunging (melee only; the tuning carries the speed).
fn lunge(def: &EnemyDef, flat: Vec2) -> MoveInput {
    match def.attack {
        EnemyAttack::Melee { .. } => MoveInput {
            wish: flat.normalize_or_zero(),
            ..MoveInput::default()
        },
        _ => MoveInput::default(),
    }
}

/// Fires one shot of a burst that had `left` (≥ 1) shots to go and moves on: to the next shot
/// `burst_gap` later, or, after the last one, back to `Chase` with the refire timer reset.
fn shoot(a: &mut Actor, def: &EnemyDef, p: &Perception, rng: &mut Rng, left: u32) -> Vec3 {
    if left > 1 {
        a.state = AiState::Attack {
            t: def.attack.burst_gap(),
            left: left - 1,
        };
    } else {
        a.refire = def.attack_refire;
        a.state = AiState::Chase;
    }
    aim(a, def, p, rng)
}

/// Where this actor's shots really start: its muzzle, unless a wall lies between the eye and
/// the muzzle (flush against a wall or jamb, the offset can poke through it). Then the start is
/// the wall hit pulled back 5 cm toward the eye. Returns the point and the sector it is in.
pub fn effective_muzzle(map: &Map, a: &Actor) -> (Vec3, SectorId) {
    let (eye, muzzle) = (a.eye(), a.muzzle());
    let reach = muzzle.distance(eye);
    let mut at = muzzle;
    if let Some(dir) = (muzzle - eye).try_normalize() {
        let ray = Ray {
            origin: eye,
            dir,
            sector: a.body.sector,
            max: reach,
        };
        if let Some(h) = crate::trace::trace_world(map, &ray)
            && h.dist <= reach
        {
            at = eye + dir * (h.dist - 0.05).max(0.0);
        }
    }
    let sector = map
        .find_sector(at.truncate(), Some(a.body.sector))
        .unwrap_or(a.body.sector);
    (at, sector)
}

/// Result of an enemy hitscan volley.
#[derive(Debug, Default, PartialEq)]
pub struct Volley {
    /// Damage of the pellets that hit the player body.
    pub player_damage: i32,
    /// Every pellet that hit the world, glass aside.
    pub impacts: Vec<Hit>,
    /// Glass walls pellets hit, once each. The volley is traced on the map as it was, so every
    /// pellet stops at a pane; the caller breaks them (`Destruct::break_glass`).
    pub glass: Vec<WallId>,
    /// Damage per actor (index, sum) from pellets that hit an actor body, ascending.
    pub actor_damage: Vec<(usize, i32)>,
}

/// Traces `pellets` pellets from the muzzle of actor `shooter` around `dir` (the aimed
/// direction, already jittered by the aim error) with `spread_deg` extra spread, up to
/// `def.attack_range`. `bodies` is `[player, actor 0, actor 1, ...]` and `alive(i)` says whether
/// actor `i` is a living target. Pellets that stop at another actor hurt it (`actor_damage`).
#[allow(clippy::too_many_arguments)]
pub fn volley(
    map: &Map,
    a: &Actor,
    shooter: usize,
    def: &EnemyDef,
    dir: Vec3,
    bodies: &[Body],
    alive: impl Fn(usize) -> bool,
    rng: &mut Rng,
) -> Volley {
    let EnemyAttack::Hitscan {
        damage,
        pellets,
        spread_deg,
    } = def.attack
    else {
        return Volley::default();
    };
    let (origin, sector) = effective_muzzle(map, a);
    let skip = |i: usize| i != 0 && (i - 1 == shooter || !alive(i - 1));
    let mut out = Volley::default();
    let mut dealt = vec![0; bodies.len()];
    for d in crate::weapons::spread_dirs(dir, pellets, spread_deg, rng) {
        let ray = Ray {
            origin,
            dir: d,
            sector,
            max: def.attack_range,
        };
        let Some(h) = trace(map, &ray, bodies, skip, 0.0) else {
            continue;
        };
        match h.kind {
            HitKind::Body(0) => out.player_damage += damage,
            HitKind::Body(b) => dealt[b] += damage,
            HitKind::Wall(w) if crate::destruct::hits_pane(map, &h) => {
                if !out.glass.contains(&w) {
                    out.glass.push(w);
                }
            }
            _ => out.impacts.push(h),
        }
    }
    out.actor_damage = (1..dealt.len())
        .filter(|&b| dealt[b] > 0)
        .map(|b| (b - 1, dealt[b]))
        .collect();
    out
}

/// Whether a swipe from `a` lands on `target`: its centre within `attack_range` plus the
/// target's radius horizontally, and the two bodies overlapping vertically.
pub fn swipe_reaches(a: &Actor, def: &EnemyDef, target: &Body) -> bool {
    let reach = def.attack_range + target.radius;
    let flat = a.body.pos.truncate().distance(target.pos.truncate());
    let z_overlap =
        a.body.pos.z < target.pos.z + target.height && target.pos.z < a.body.pos.z + a.body.height;
    flat <= reach && z_overlap
}

/// Bolt direction from the muzzle to the player's chest, jittered by the aim error.
fn aim(a: &Actor, def: &EnemyDef, p: &Perception, rng: &mut Rng) -> Vec3 {
    let ideal = (p.chest - a.muzzle()).normalize_or(Vec3::X);
    crate::weapons::spread_dirs(ideal, 1, def.aim_error_deg, rng)[0]
}

/// Chase movement: straight at a visible (or same-sector) player, otherwise toward the next
/// sector on the route, strafing while the player is in sight.
fn chase(
    a: &mut Actor,
    def: &EnemyDef,
    map: &Map,
    p: &Perception,
    sees: bool,
    rng: &mut Rng,
    dt: f32,
) -> MoveInput {
    let here = a.body.pos.truncate();
    let player = p.eye.truncate();
    a.repath -= dt;
    let goal = if sees || a.body.sector == p.sector {
        a.hop = None;
        Some(player)
    } else {
        let stale = a
            .hop
            .is_none_or(|h| portal_waypoint(map, a.body.sector, h).is_none());
        if a.repath <= 0.0 || stale {
            let pass = match a.locomotion {
                Locomotion::Fly { .. } => Pass::Fly { height: def.height },
                _ => Pass::Walk(def.tuning()),
            };
            a.hop = next_hop(map, a.body.sector, p.sector, pass);
        }
        a.hop.and_then(|h| portal_waypoint(map, a.body.sector, h))
    };
    let Some(goal) = goal else {
        a.corner = None;
        if a.repath <= 0.0 {
            a.repath = REPATH;
        }
        return MoveInput::default(); // unreachable: wait
    };
    // Steer around pillars and inside corners: keep the chosen corner until the next repath,
    // or until it is reached or no longer in sight.
    let (sector, radius) = (a.body.sector, def.radius);
    if steer::line_clear(map, sector, here, goal, radius) {
        a.corner = None;
    } else {
        let keep = a.corner.filter(|&c| {
            c.distance(here) > CORNER_REACHED
                && steer::line_clear(map, sector, here, c, radius * steer::CORNER_SLACK)
        });
        let best = if keep.is_none() || a.repath <= 0.0 {
            steer::steer_corner(map, sector, here, goal, radius)
        } else {
            None
        };
        // Re-evaluating on repath must not flip between near-equal corners (dithering).
        let cost = |c: Vec2| here.distance(c) + c.distance(goal);
        a.corner = match (keep, best) {
            (Some(k), Some(b)) if cost(k) <= cost(b) + CORNER_STICKY => Some(k),
            (k, b) => b.or(k),
        };
    }
    if a.repath <= 0.0 {
        a.repath = REPATH;
    }
    let target = a.corner.unwrap_or(goal);
    let to = goal - here;
    let dir = (target - here).normalize_or_zero();
    if dir != Vec2::ZERO {
        a.angle = heading(dir);
    }
    let mut wish = if sees && to.length() <= CLOSE_ENOUGH {
        Vec2::ZERO
    } else {
        dir
    };
    if sees && def.strafe && dir != Vec2::ZERO {
        let left = a.strafe.abs() - dt;
        if a.strafe == 0.0 || left <= 0.0 {
            let sign = if a.strafe == 0.0 {
                if rng.chance(0.5) { 1.0 } else { -1.0 }
            } else {
                -a.strafe.signum()
            };
            let (lo, hi) = STRAFE_FLIP;
            a.strafe = sign * (lo + rng.unit() * (hi - lo));
        } else {
            a.strafe = a.strafe.signum() * left;
        }
        wish += dir.perp() * STRAFE * a.strafe.signum();
    }
    MoveInput {
        wish: wish.clamp_length_max(1.0),
        ..MoveInput::default()
    }
}

/// Just past the midpoint of the widest portal wall from sector `from` into `to`, or `None` if
/// the two sectors share no portal (intact glass does not count).
fn portal_waypoint(map: &Map, from: SectorId, to: SectorId) -> Option<Vec2> {
    let wall = map.sectors[from]
        .walls()
        .map(|w| &map.walls[w])
        .filter(|w| w.passage() == Some(to))
        .max_by(|x, y| x.a.distance(x.b).total_cmp(&y.a.distance(y.b)))?;
    Some((wall.a + wall.b) * 0.5 - wall.inward_normal() * PORTAL_OVERSHOOT)
}

/// First sector on the shortest (fewest portals) walkable route from `from` to `to`, judged on
/// live floor and ceiling heights with `Pass::allows` (`can_cross` for walkers, the opening for flyers), never through intact glass. `None` if
/// `from == to` or `to` is unreachable.
pub fn next_hop(map: &Map, from: SectorId, to: SectorId, pass: Pass) -> Option<SectorId> {
    if from == to {
        return None;
    }
    let pose = |s: SectorId| (map.sectors[s].floor_z, map.sectors[s].ceil_z);
    // `first[s]` is the first hop on the route to `s`.
    let mut first: Vec<Option<SectorId>> = vec![None; map.sectors.len()];
    let mut seen = vec![false; map.sectors.len()];
    seen[from] = true;
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(s) = queue.pop_front() {
        for n in map.passages(s) {
            if seen[n] || !pass.allows(pose(s), pose(n)) {
                continue;
            }
            seen[n] = true;
            first[n] = if s == from { Some(n) } else { first[s] };
            if n == to {
                return first[n];
            }
            queue.push_back(n);
        }
    }
    None
}

/// Applies `n` damage. A kill sends the actor to `Dying` from any state; a non-killing hit wakes
/// a sleeping or alert actor into `Chase` and then, with `def.pain_chance`, into `Pain`. Dying
/// and dead actors ignore hits, as does a hit of `n <= 0`.
pub fn hurt(a: &mut Actor, def: &EnemyDef, n: i32, rng: &mut Rng) -> DamageOutcome {
    if !a.alive() {
        return DamageOutcome::Ignored;
    }
    let outcome = a.health.damage(n);
    match outcome {
        DamageOutcome::Killed => {
            a.state = AiState::Dying { t: def.death_time };
            a.body.height = CORPSE_HEIGHT;
            a.hop = None;
        }
        // Static actors (barrels) neither wake nor flinch.
        DamageOutcome::Hurt if a.locomotion == Locomotion::Static => {}
        DamageOutcome::Hurt => {
            if matches!(a.state, AiState::Sleep | AiState::Alert { .. }) {
                a.state = AiState::Chase;
            }
            let flinch = !matches!(a.state, AiState::Pain { .. }) && a.pain_ready_in <= 0.0;
            if flinch && rng.chance(def.pain_chance) {
                a.state = AiState::Pain { t: def.pain_time };
            }
        }
        DamageOutcome::Ignored => {}
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::difficulty::Difficulty;
    use crate::fixtures::{defs, door_rooms, pillar_room};
    use crate::mechanics::Mechanics;
    use crate::movement::step_player;
    use std::f32::consts::PI;

    const DT: f32 = 1.0 / 60.0;

    fn grunt() -> EnemyDef {
        defs().enemy(ActorKind::Grunt).clone()
    }

    fn actor_at(map: &Map, def: &EnemyDef, x: f32, y: f32, angle: f32, asleep: bool) -> Actor {
        let spawn = ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(x, y),
            angle,
            asleep,
            skill: Difficulty::Easy,
            on_death: None,
        };
        Actor::new(map, def, &spawn).expect("spawn inside the map")
    }

    /// A standing player (1.8 m) at `(x, y)`.
    fn player_at(map: &Map, x: f32, y: f32) -> Perception {
        let b = Body::spawn(map, Vec2::new(x, y), 0.35, 1.8).unwrap();
        Perception {
            eye: b.pos + Vec3::Z * 1.6,
            chest: b.pos + Vec3::Z * 0.6 * b.height,
            sector: b.sector,
            alive: true,
        }
    }

    /// Runs `think` and moves the body like the combat orchestrator will. Returns fire directions
    /// with the tick index at which they were fired.
    fn run(
        a: &mut Actor,
        def: &EnemyDef,
        map: &Map,
        p: &Perception,
        rng: &mut Rng,
        ticks: usize,
    ) -> Vec<(usize, Vec3)> {
        let t = def.tuning();
        let mut fired = Vec::new();
        for i in 0..ticks {
            let (input, fire) = think(a, def, map, p, rng, DT);
            if a.alive() {
                a.prev_pos = a.body.pos;
                step_player(map, &mut a.body, &input, &t, DT);
            }
            if let Some(d) = fire {
                fired.push((i, d));
            }
        }
        fired
    }

    /// An L-shaped route with no line of sight end to end: room A x,y∈[0,4] (sector 0), a 2 m
    /// corridor B x∈[4,8], y∈[0,2] (sector 1) and room C x∈[6,8], y∈[2,6] (sector 2).
    fn corridor() -> Map {
        Map::from_ron(
            r#"(
            name: "corridor",
            materials: ["wall", "floor", "ceiling"],
            vertices: [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (4.0, 4.0), (0.0, 4.0),
                       (8.0, 0.0), (8.0, 2.0), (6.0, 2.0), (8.0, 6.0), (6.0, 6.0)],
            sectors: [
                (loops: [[0, 1, 2, 3, 4]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
                (loops: [[1, 5, 6, 7, 2]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
                (loops: [[7, 6, 8, 9]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            ],
            player_start: (pos: (1.0, 1.0), angle_deg: 0.0),
        )"#,
        )
        .expect("corridor fixture is valid")
    }

    #[test]
    fn sleeping_grunt_ignores_player_behind_it() {
        let map = pillar_room();
        let def = grunt();
        // Facing +x; the player is 6 m straight behind along a clear line.
        let mut a = actor_at(&map, &def, 8.0, 8.0, 0.0, true);
        let p = player_at(&map, 2.0, 8.0);
        let mut rng = Rng::new(1);
        for _ in 0..120 {
            let (input, fire) = think(&mut a, &def, &map, &p, &mut rng, DT);
            assert_eq!(input, MoveInput::default());
            assert!(fire.is_none());
        }
        assert_eq!(a.state, AiState::Sleep);
    }

    #[test]
    fn sleeping_grunt_wakes_on_sight_in_fov() {
        let map = pillar_room();
        let def = grunt();
        let p = player_at(&map, 2.0, 8.0);
        let mut rng = Rng::new(1);
        // Facing the player.
        let mut a = actor_at(&map, &def, 8.0, 8.0, PI, true);
        think(&mut a, &def, &map, &p, &mut rng, DT);
        assert!(matches!(a.state, AiState::Alert { .. }), "{:?}", a.state);
        // Just inside the FOV edge (60° off a 120° FOV, minus a little) still wakes.
        let edge = PI - (def.fov_deg / 2.0 - 2.0).to_radians();
        let mut a = actor_at(&map, &def, 8.0, 8.0, edge, true);
        think(&mut a, &def, &map, &p, &mut rng, DT);
        assert!(matches!(a.state, AiState::Alert { .. }), "{:?}", a.state);
        // Just outside it does not.
        let out = PI - (def.fov_deg / 2.0 + 2.0).to_radians();
        let mut a = actor_at(&map, &def, 8.0, 8.0, out, true);
        think(&mut a, &def, &map, &p, &mut rng, DT);
        assert_eq!(a.state, AiState::Sleep);
        // Facing the player but with the pillar in the way: stays asleep.
        let hidden = player_at(&map, 2.0, 5.0);
        let mut a = actor_at(&map, &def, 8.0, 5.0, PI, true);
        think(&mut a, &def, &map, &hidden, &mut rng, DT);
        assert_eq!(a.state, AiState::Sleep);
        // Facing the player, in plain sight, but beyond sight range: stays asleep.
        let mut short = def.clone();
        short.sight_range = 5.0;
        let mut a = actor_at(&map, &short, 8.0, 8.0, PI, true);
        think(&mut a, &short, &map, &p, &mut rng, DT);
        assert_eq!(a.state, AiState::Sleep);
    }

    #[test]
    fn wake_moves_only_sleepers_to_alert() {
        let map = pillar_room();
        let def = grunt();
        let mut a = actor_at(&map, &def, 8.0, 8.0, 0.0, true);
        wake(&mut a);
        assert_eq!(a.state, AiState::Alert { t: 0.0 });
        a.state = AiState::Chase;
        wake(&mut a);
        assert_eq!(a.state, AiState::Chase);
    }

    #[test]
    fn alert_waits_reaction_then_chases() {
        let map = pillar_room();
        let def = grunt();
        // Player out of sight behind the pillar, so only the timer matters.
        let p = player_at(&map, 2.0, 5.0);
        let mut a = actor_at(&map, &def, 8.0, 5.0, PI, true);
        wake(&mut a);
        let mut rng = Rng::new(3);
        let ticks = (def.reaction / DT).round() as usize;
        for _ in 0..ticks - 1 {
            let (input, fire) = think(&mut a, &def, &map, &p, &mut rng, DT);
            assert_eq!(input.wish, Vec2::ZERO, "alert actors stand still");
            assert!(fire.is_none());
            assert!(matches!(a.state, AiState::Alert { .. }), "{:?}", a.state);
        }
        think(&mut a, &def, &map, &p, &mut rng, DT);
        think(&mut a, &def, &map, &p, &mut rng, DT);
        assert_eq!(a.state, AiState::Chase);
    }

    #[test]
    fn chase_routes_through_portal_sectors() {
        let map = corridor();
        let def = grunt();
        let t = def.tuning();
        assert_eq!(next_hop(&map, 0, 2, Pass::Walk(t)), Some(1));
        assert_eq!(next_hop(&map, 1, 2, Pass::Walk(t)), Some(2));
        assert_eq!(next_hop(&map, 2, 0, Pass::Walk(t)), Some(1));
        assert_eq!(next_hop(&map, 1, 1, Pass::Walk(t)), None);

        let p = player_at(&map, 7.0, 5.0);
        let mut a = actor_at(&map, &def, 1.0, 1.0, 0.0, false);
        assert!(!crate::trace::can_see(&map, a.eye(), a.body.sector, p.eye));
        a.state = AiState::Chase;
        let mut rng = Rng::new(5);
        let mut visited = vec![a.body.sector];
        for _ in 0..600 {
            run(&mut a, &def, &map, &p, &mut rng, 1);
            if visited.last() != Some(&a.body.sector) {
                visited.push(a.body.sector);
            }
            if a.body.sector == 2 {
                break;
            }
        }
        assert_eq!(visited, vec![0, 1, 2], "route taken");
    }

    #[test]
    fn chase_does_not_path_through_closed_door() {
        let def = grunt();
        let t = def.tuning();
        let authored = door_rooms("(kind: Door)", "");
        assert_eq!(
            next_hop(&authored, 0, 2, Pass::Walk(t)),
            Some(1),
            "open door is passable"
        );

        let mut map = door_rooms("(kind: Door)", "");
        let _mech = Mechanics::new(&mut map);
        assert_eq!(map.sectors[1].ceil_z, map.sectors[1].floor_z);
        assert_eq!(next_hop(&map, 0, 2, Pass::Walk(t)), None);
        assert_eq!(next_hop(&map, 0, 1, Pass::Walk(t)), None);

        let p = player_at(&map, 6.5, 2.0);
        let mut a = actor_at(&map, &def, 2.0, 2.0, 0.0, false);
        a.state = AiState::Chase;
        let mut rng = Rng::new(9);
        let fired = run(&mut a, &def, &map, &p, &mut rng, 180);
        assert!(fired.is_empty());
        assert_eq!(a.body.sector, 0);
        assert_eq!(a.state, AiState::Chase);
        assert!(a.body.vel.truncate().length() < 1e-3, "stands at the door");
    }

    #[test]
    fn attack_fires_burst_then_cooldown() {
        let map = pillar_room();
        let mut def = grunt();
        def.strafe = false;
        let EnemyAttack::Bolts { proj, .. } = def.attack else {
            panic!("the Grunt shoots bolts")
        };
        def.attack = EnemyAttack::Bolts {
            proj,
            burst: 3,
            burst_gap: 0.25,
        };
        let (burst, burst_gap) = (def.attack.burst(), def.attack.burst_gap());
        let p = player_at(&map, 2.0, 8.0);
        let mut a = actor_at(&map, &def, 8.0, 8.0, PI, false);
        a.state = AiState::Chase;
        let mut rng = Rng::new(11);
        let total = ((2.0 * def.attack_refire + 1.0) / DT) as usize;
        let mut fired = Vec::new();
        let mut attack_ticks = 0;
        let t = def.tuning();
        for i in 0..total {
            let (input, fire) = think(&mut a, &def, &map, &p, &mut rng, DT);
            if matches!(a.state, AiState::Attack { .. }) {
                attack_ticks += 1;
                assert_eq!(input.wish, Vec2::ZERO, "attackers stand still");
            }
            step_player(&map, &mut a.body, &input, &t, DT);
            if let Some(d) = fire {
                fired.push((i, d, a.muzzle()));
            }
        }
        assert!(attack_ticks > 0);
        assert!(fired.len() >= 2 * burst as usize, "{} shots", fired.len());
        let gap = (burst_gap / DT).round() as usize;
        let refire = (def.attack_refire / DT).round() as usize;
        // First burst: `burst` shots `burst_gap` apart.
        for w in fired[..burst as usize].windows(2) {
            assert!(
                (w[1].0 - w[0].0).abs_diff(gap) <= 1,
                "{} → {}",
                w[0].0,
                w[1].0
            );
        }
        // Then nothing until the refire timer has run.
        let pause = fired[burst as usize].0 - fired[burst as usize - 1].0;
        assert!(pause + 1 >= refire, "pause {pause} ticks, refire {refire}");
        // Every bolt is a unit vector aimed at the chest, within the aim error.
        let max_err = def.aim_error_deg.to_radians() + 1e-3;
        for (_, d, muzzle) in &fired {
            assert!((d.length() - 1.0).abs() < 1e-4);
            let ideal = (p.chest - *muzzle).normalize();
            assert!(
                d.angle_between(ideal) <= max_err,
                "off by {}",
                d.angle_between(ideal)
            );
        }
    }

    #[test]
    fn kill_during_pain_goes_to_dying() {
        let map = pillar_room();
        let mut def = grunt();
        def.pain_chance = 1.0;
        let mut rng = Rng::new(2);
        let mut a = actor_at(&map, &def, 8.0, 8.0, 0.0, true);
        assert_eq!(hurt(&mut a, &def, 1, &mut rng), DamageOutcome::Hurt);
        assert_eq!(a.state, AiState::Pain { t: def.pain_time });
        assert_eq!(hurt(&mut a, &def, 1000, &mut rng), DamageOutcome::Killed);
        assert_eq!(a.state, AiState::Dying { t: def.death_time });
        assert_eq!(a.body.height, CORPSE_HEIGHT);
        assert!(!a.alive());
        // Dying runs out into Dead, motionless and silent throughout.
        let p = player_at(&map, 2.0, 8.0);
        let ticks = (def.death_time / DT).ceil() as usize + 1;
        for _ in 0..ticks {
            let (input, fire) = think(&mut a, &def, &map, &p, &mut rng, DT);
            assert_eq!(input, MoveInput::default());
            assert!(fire.is_none());
        }
        assert_eq!(a.state, AiState::Dead);
        // Pain itself ends in Chase.
        let ticks = (def.pain_time / DT).ceil() as usize + 1;
        let hidden = player_at(&map, 2.0, 5.0);
        let mut c = actor_at(&map, &def, 8.0, 5.0, 0.0, true);
        hurt(&mut c, &def, 1, &mut rng);
        for _ in 0..ticks {
            let (input, fire) = think(&mut c, &def, &map, &hidden, &mut rng, DT);
            assert!(fire.is_none());
            if matches!(c.state, AiState::Pain { .. }) {
                assert_eq!(input.wish, Vec2::ZERO, "pain freezes");
            }
        }
        assert_eq!(c.state, AiState::Chase);
    }

    #[test]
    fn pain_is_seeded_and_bounded() {
        let map = pillar_room();
        let def = grunt();
        let outcomes = |seed: u64, def: &EnemyDef| -> Vec<bool> {
            let mut rng = Rng::new(seed);
            (0..2000)
                .map(|_| {
                    let mut a = actor_at(&map, def, 8.0, 8.0, 0.0, true);
                    assert_eq!(hurt(&mut a, def, 1, &mut rng), DamageOutcome::Hurt);
                    match a.state {
                        AiState::Pain { t } => {
                            assert_eq!(t, def.pain_time);
                            true
                        }
                        AiState::Chase => false,
                        s => panic!("hurt sleeper ended in {s:?}"),
                    }
                })
                .collect()
        };
        let a = outcomes(77, &def);
        assert_eq!(a, outcomes(77, &def), "same seed, same outcomes");
        assert_ne!(a, outcomes(78, &def));
        let rate = a.iter().filter(|&&p| p).count() as f32 / a.len() as f32;
        assert!(
            (rate - def.pain_chance).abs() < 0.05,
            "pain rate {rate} vs chance {}",
            def.pain_chance
        );
        let mut never = def.clone();
        never.pain_chance = 0.0;
        assert!(outcomes(5, &never).iter().all(|&p| !p));
        let mut always = def.clone();
        always.pain_chance = 1.0;
        assert!(outcomes(5, &always).iter().all(|&p| p));
    }

    #[test]
    fn dead_actor_is_ignored_by_hurt() {
        let map = pillar_room();
        let def = grunt();
        let mut rng = Rng::new(4);
        let mut a = actor_at(&map, &def, 8.0, 8.0, 0.0, false);
        assert_eq!(hurt(&mut a, &def, 0, &mut rng), DamageOutcome::Ignored);
        assert_eq!(a.state, AiState::Alert { t: 0.0 }, "no damage, no reaction");
        assert_eq!(
            hurt(&mut a, &def, def.health, &mut rng),
            DamageOutcome::Killed
        );
        let dying = a.clone();
        assert_eq!(hurt(&mut a, &def, 10, &mut rng), DamageOutcome::Ignored);
        assert_eq!(a, dying);
        a.state = AiState::Dead;
        let dead = a.clone();
        assert_eq!(hurt(&mut a, &def, 10, &mut rng), DamageOutcome::Ignored);
        assert_eq!(a, dead);
    }

    #[test]
    fn actors_idle_when_player_dead() {
        let map = pillar_room();
        let def = grunt();
        let mut p = player_at(&map, 2.0, 8.0);
        p.alive = false;
        let mut rng = Rng::new(6);
        let mut chasing = actor_at(&map, &def, 8.0, 8.0, PI, false);
        chasing.state = AiState::Chase;
        let mut attacking = actor_at(&map, &def, 8.0, 8.0, PI, false);
        attacking.state = AiState::Attack { t: 0.0, left: 2 };
        let mut sleeping = actor_at(&map, &def, 8.0, 8.0, PI, true);
        for a in [&mut chasing, &mut attacking, &mut sleeping] {
            for _ in 0..120 {
                let (input, fire) = think(a, &def, &map, &p, &mut rng, DT);
                assert_eq!(input, MoveInput::default());
                assert!(fire.is_none());
            }
        }
        assert_eq!(sleeping.state, AiState::Sleep, "a dead player wakes nobody");
    }

    #[test]
    fn pain_cooldown_prevents_stun_lock() {
        let map = pillar_room();
        let mut def = grunt();
        def.pain_chance = 1.0;
        def.health = 10_000;
        let mut rng = Rng::new(9);
        let p = player_at(&map, 8.0, 5.0);
        let mut a = actor_at(&map, &def, 8.0, 8.0, -PI / 2.0, false);
        a.state = AiState::Chase;
        let mut fired = None;
        let mut pains = 0;
        let mut was_pain = false;
        for i in 0..120 {
            if i % 6 == 0 {
                hurt(&mut a, &def, 1, &mut rng); // chaingun cadence
            }
            let (input, fire) = think(&mut a, &def, &map, &p, &mut rng, DT);
            let pain = matches!(a.state, AiState::Pain { .. });
            pains += (pain && !was_pain) as u32;
            was_pain = pain;
            let _ = input;
            if fire.is_some() && fired.is_none() {
                fired = Some(i);
            }
        }
        assert!(fired.is_some(), "stun-locked: never fired in 2 s");
        assert!(pains <= 2, "{pains} pains in 2 s");
    }

    #[test]
    fn pain_rolls_again_after_the_cooldown() {
        let map = pillar_room();
        let mut def = grunt();
        def.pain_chance = 1.0;
        def.health = 10_000;
        let mut rng = Rng::new(3);
        let p = player_at(&map, 2.0, 5.0); // hidden behind the pillar: nothing fires
        let mut a = actor_at(&map, &def, 8.0, 5.0, 0.0, false);
        a.state = AiState::Chase;
        hurt(&mut a, &def, 1, &mut rng);
        assert!(matches!(a.state, AiState::Pain { .. }));
        for _ in 0..(def.pain_time / DT).ceil() as usize + 1 {
            think(&mut a, &def, &map, &p, &mut rng, DT);
        }
        assert_eq!(a.state, AiState::Chase);
        assert!(a.pain_ready_in > 0.0);
        hurt(&mut a, &def, 1, &mut rng);
        assert_eq!(a.state, AiState::Chase, "still cooling down");
        for _ in 0..(def.pain_cooldown / DT).ceil() as usize + 2 {
            think(&mut a, &def, &map, &p, &mut rng, DT);
        }
        hurt(&mut a, &def, 1, &mut rng);
        assert!(matches!(a.state, AiState::Pain { .. }));
    }

    fn chase_until_sees(
        map: &Map,
        def: &EnemyDef,
        a: &mut Actor,
        p: &Perception,
        max: usize,
    ) -> bool {
        let mut rng = Rng::new(1);
        for _ in 0..max {
            run(a, def, map, p, &mut rng, 1);
            if crate::trace::can_see(map, a.eye(), a.body.sector, p.eye) {
                return true;
            }
        }
        false
    }

    #[test]
    fn actor_steers_around_pillar() {
        let map = pillar_room();
        let def = grunt();
        let p = player_at(&map, 8.0, 5.0);
        let mut a = actor_at(&map, &def, 2.0, 5.0, 0.0, false);
        a.state = AiState::Chase;
        assert!(
            chase_until_sees(&map, &def, &mut a, &p, 480),
            "stuck at {:?} {:?} {:?}",
            a.body.pos,
            a.state,
            a.corner
        );
    }

    #[test]
    fn flyer_steers_around_pillar() {
        let map = pillar_room();
        let def = defs().enemy(ActorKind::Drone).clone();
        let spawn = ActorSpawn {
            kind: ActorKind::Drone,
            pos: Vec2::new(2.0, 5.0),
            angle: 0.0,
            asleep: false,
            skill: Difficulty::Easy,
            on_death: None,
        };
        let mut a = Actor::new(&map, &def, &spawn).unwrap();
        a.state = AiState::Chase;
        let p = player_at(&map, 8.0, 5.0);
        assert!(
            chase_until_sees(&map, &def, &mut a, &p, 480),
            "stuck at {:?} {:?} {:?}",
            a.body.pos,
            a.state,
            a.corner
        );
    }

    #[test]
    fn actor_reaches_target_around_l_corner() {
        let map = Map::from_ron(
            r#"(
            name: "L",
            materials: ["wall", "floor", "ceiling"],
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0)],
            sectors: [
                (loops: [[0, 1, 2, 3, 4, 5]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            ],
            player_start: (pos: (1.0, 1.0), angle_deg: 0.0),
        )"#,
        )
        .unwrap();
        let def = grunt();
        let p = player_at(&map, 2.0, 9.0);
        let mut a = actor_at(&map, &def, 9.0, 2.0, 0.0, false);
        a.state = AiState::Chase;
        assert!(!crate::trace::can_see(&map, a.eye(), a.body.sector, p.eye));
        assert!(
            chase_until_sees(&map, &def, &mut a, &p, 900),
            "stuck at {:?} {:?} {:?}",
            a.body.pos,
            a.state,
            a.corner
        );
    }
}
