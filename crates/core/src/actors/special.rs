//! Boss special moves, run in place of `think` while they last: burrowing between the level's
//! burrow points (`Map::burrow_points`) and the ground pound. Which moves an actor has comes
//! from its (phased) def: `EnemyDef::burrow` and `EnemyDef::pound`.
//!
//! A burrow runs Chase → `Burrow` (digging, still a target) → `Under` (no target, no collision)
//! → `Emerge` at another burrow point → Chase. A pound runs Chase → `Pound` (the wind-up) → the
//! slam → Chase. Both start only from `Chase` with the actor on the ground; their timers
//! (`Actor::burrow_in`, `Actor::pound_in`) count down while it chases or attacks. The caller
//! (`Combat::tick`) resolves what a move reports: the minions of a surfacing and the pound's
//! damage.

use super::{Actor, AiState, Perception, heading};
use crate::collide::Body;
use crate::defs::{EnemyDef, Locomotion, PoundDef};
use crate::map::Map;
use crate::rng::Rng;
use glam::Vec2;

/// A burrowing actor comes up at least this far (m) from the player, unless every other point
/// is closer.
pub const PLAYER_CLEAR: f32 = 5.0;
/// An actor that has not come up at a burrow point yet stands on one within this distance (m).
const AT_POINT: f32 = 1.0;

/// What `step` did with the actor this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Special {
    /// No special move: run `think` as usual.
    Think,
    /// A special move holds the actor (it neither moves nor attacks), maybe with something for
    /// the caller to resolve.
    Busy(Option<Move>),
}

/// Something a special move did that the caller resolves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Started digging down.
    Burrowed,
    /// Came up (at `Actor::burrow_at`); up to `spawn` minions come up beside it.
    Surfaced { spawn: u32 },
    /// Slammed the floor.
    Pound(PoundDef),
}

/// One tick of special moves for `a` (already phased `def`), before `think`. Dying and dead
/// actors, static ones and every actor while its target is dead are left to `think`.
pub fn step(
    a: &mut Actor,
    def: &EnemyDef,
    map: &Map,
    p: &Perception,
    rng: &mut Rng,
    dt: f32,
) -> Special {
    if !a.alive() || a.locomotion == Locomotion::Static || !p.alive {
        return Special::Think;
    }
    match a.state {
        AiState::Burrow { t, plan } => {
            a.state = if t - dt <= 0.0 {
                AiState::Under {
                    t: plan.under,
                    plan,
                }
            } else {
                AiState::Burrow { t: t - dt, plan }
            };
            Special::Busy(None)
        }
        AiState::Under { t, plan } => {
            if t - dt > 0.0 {
                a.state = AiState::Under { t: t - dt, plan };
                return Special::Busy(None);
            }
            surface(a, map, p, rng);
            a.state = AiState::Emerge { t: plan.rise };
            Special::Busy(Some(Move::Surfaced { spawn: plan.spawn }))
        }
        AiState::Emerge { t } => {
            a.state = if t - dt <= 0.0 {
                AiState::Chase
            } else {
                AiState::Emerge { t: t - dt }
            };
            Special::Busy(None)
        }
        AiState::Pound { t, plan } => {
            face(a, p);
            if t - dt > 0.0 {
                a.state = AiState::Pound { t: t - dt, plan };
                return Special::Busy(None);
            }
            a.state = AiState::Chase;
            a.pound_in = plan.every;
            Special::Busy(Some(Move::Pound(plan)))
        }
        AiState::Chase | AiState::Attack { .. } => {
            if def.burrow.is_some() {
                a.burrow_in = (a.burrow_in - dt).max(0.0);
            }
            if def.pound.is_some() {
                a.pound_in = (a.pound_in - dt).max(0.0);
            }
            if a.state != AiState::Chase || !a.body.on_ground {
                return Special::Think;
            }
            if let Some(plan) = def.burrow
                && a.burrow_in <= 0.0
                && map.burrow_points.len() >= 2
            {
                let (lo, hi) = plan.every;
                a.burrow_in = lo + rng.unit() * (hi - lo);
                a.state = AiState::Burrow { t: plan.dig, plan };
                a.hop = None;
                a.corner = None;
                return Special::Busy(Some(Move::Burrowed));
            }
            if let Some(plan) = def.pound
                && a.pound_in <= 0.0
                && a.body.pos.truncate().distance(p.eye.truncate()) <= plan.radius
            {
                face(a, p);
                a.state = AiState::Pound {
                    t: plan.windup,
                    plan,
                };
                return Special::Busy(None);
            }
            Special::Think
        }
        _ => Special::Think,
    }
}

/// Turns the actor toward its target.
fn face(a: &mut Actor, p: &Perception) {
    let d = p.eye.truncate() - a.body.pos.truncate();
    if d != Vec2::ZERO {
        a.angle = heading(d);
    }
}

/// The burrow point a burrowed actor comes up at: never the one it went down at (the one it
/// last came up at, else one it stands on), preferably one at least `PLAYER_CLEAR` from the
/// player, picked with `rng` among the candidates. `None` with fewer than two points.
pub fn pick_point(map: &Map, a: &Actor, player: Vec2, rng: &mut Rng) -> Option<usize> {
    let pts = &map.burrow_points;
    if pts.len() < 2 {
        return None;
    }
    let here = a.burrow_at.or_else(|| {
        let at = a.body.pos.truncate();
        (0..pts.len())
            .filter(|&i| pts[i].distance(at) <= AT_POINT)
            .min_by(|&i, &j| pts[i].distance(at).total_cmp(&pts[j].distance(at)))
    });
    let others: Vec<usize> = (0..pts.len()).filter(|&i| Some(i) != here).collect();
    let far: Vec<usize> = others
        .iter()
        .copied()
        .filter(|&i| pts[i].distance(player) >= PLAYER_CLEAR)
        .collect();
    let pool = if far.is_empty() { others } else { far };
    // At least one other point is left: there are two or more.
    let k = ((rng.unit() * pool.len() as f32) as usize).min(pool.len() - 1);
    Some(pool[k])
}

/// Moves a burrowed actor to the point `pick_point` chooses, facing the player. It stays where
/// it went down when there is no point to go to (or the point lies outside every sector).
fn surface(a: &mut Actor, map: &Map, p: &Perception, rng: &mut Rng) {
    let Some(k) = pick_point(map, a, p.eye.truncate(), rng) else {
        return;
    };
    let Some(body) = Body::spawn(map, map.burrow_points[k], a.body.radius, a.body.height) else {
        return;
    };
    a.body = body;
    a.prev_pos = body.pos;
    a.burrow_at = Some(k);
    a.hop = None;
    a.corner = None;
    a.repath = 0.0;
    face(a, p);
}
