//! What the bosses' special moves (`actors::special`) do to the rest of the fight: the minions
//! a surfacing burrower brings up and the ground pound's shockwave.

use super::{Combat, CombatEvent, P0, PlayerTarget, hurt_player};
use crate::actors::special::Move;
use crate::actors::{Actor, Target};
use crate::collide::Body;
use crate::defs::{Defs, PoundDef};
use crate::difficulty::Difficulty;
use crate::geom::closest_point_on_segment;
use crate::map::{ActorSpawn, Map, SectorId};
use crate::movement::Tuning;
use crate::projectile::Shooter;
use crate::trace::can_see;
use glam::{Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Gap (m) between a surfacing burrower and the minions it brings up.
const MINION_GAP: f32 = 0.5;
/// Where minions come up, as turns from the burrower's heading: beside it first, then behind,
/// then ahead.
const MINION_SIDES: [f32; 8] = [
    FRAC_PI_2,
    -FRAC_PI_2,
    3.0 * FRAC_PI_4,
    -3.0 * FRAC_PI_4,
    PI,
    FRAC_PI_4,
    -FRAC_PI_4,
    0.0,
];
/// A pound reaches only bodies whose feet are within this height (m) of the pounder's.
pub const POUND_REACH_Z: f32 = 0.75;
/// Height (m) above the feet at which the shockwave's path (and a minion's way out) is traced.
const LOW_TRACE: f32 = 0.5;

impl Combat {
    /// Resolves a special move actor `i` made this tick.
    pub(super) fn special_move(
        &mut self,
        map: &Map,
        defs: &Defs,
        i: usize,
        m: Move,
        player: &mut PlayerTarget,
        out: &mut Vec<CombatEvent>,
    ) {
        match m {
            Move::Burrowed => out.push(CombatEvent::Burrowed { actor: i }),
            Move::Surfaced { spawn } => {
                out.push(CombatEvent::Surfaced { actor: i });
                self.spawn_minions(map, defs, i, spawn, player.body, out);
            }
            Move::Pound(plan) => self.ground_pound(map, defs, i, plan, player, out),
        }
    }

    /// How many minions actor `i` brought up are still alive.
    pub fn live_minions(&self, i: usize) -> usize {
        self.actors
            .iter()
            .filter(|a| a.summoner == Some(i) && a.alive())
            .count()
    }

    /// Up to `n` awake minions (the def's `minions`) come up beside actor `i`, facing the
    /// player, as long as its live minions stay within the skill's cap. Each goes on a free
    /// spot of the same floor that the burrower can see at knee height; spots that do not fit
    /// are skipped. Reports `ActorSpawned` per minion.
    pub(super) fn spawn_minions(
        &mut self,
        map: &Map,
        defs: &Defs,
        i: usize,
        n: u32,
        player: &Body,
        out: &mut Vec<CombatEvent>,
    ) {
        let Some(m) = defs.enemy(self.actors[i].kind).minions else {
            return;
        };
        let cap = m.cap.get(self.skill) as usize;
        let mut left = (n as usize).min(cap.saturating_sub(self.live_minions(i)));
        let def = defs.enemy(m.kind);
        let q = &self.actors[i].body;
        let (centre, sector) = (q.pos, q.sector);
        let reach = q.radius + def.radius + MINION_GAP;
        let angle = self.actors[i].angle;
        for turn in MINION_SIDES {
            if left == 0 {
                break;
            }
            let at = centre.truncate() + Vec2::from_angle(angle + turn) * reach;
            let mut bodies = vec![*player];
            bodies.extend(
                self.actors
                    .iter()
                    .filter(|a| a.targetable())
                    .map(|a| a.body),
            );
            if !spot_free(map, centre, sector, at, def.radius, def.height, &bodies) {
                continue;
            }
            let to_player = player.pos.truncate() - at;
            let spawn = ActorSpawn {
                kind: m.kind,
                pos: at,
                angle: to_player.y.atan2(to_player.x),
                asleep: false,
                skill: Difficulty::Easy,
                on_death: None,
            };
            let Some(mut a) = Actor::new(map, def, &spawn) else {
                continue;
            };
            a.summoner = Some(i);
            a.target = Target::default();
            self.actors.push(a);
            out.push(CombatEvent::ActorSpawned(self.actors.len() - 1));
            left -= 1;
        }
    }

    /// Actor `i` slams the floor (`GroundPound`): every body on the ground (the player and the
    /// other targetable actors) whose feet are within `POUND_REACH_Z` of its own height, within
    /// `plan.radius` horizontally and in its line at knee height takes `plan.damage` falling
    /// off linearly to nothing at the radius. Jumping, jetpacking or flying bodies are not on
    /// the ground and take none. The player's share goes through the damage scale.
    pub(super) fn ground_pound(
        &mut self,
        map: &Map,
        defs: &Defs,
        i: usize,
        plan: PoundDef,
        player: &mut PlayerTarget,
        out: &mut Vec<CombatEvent>,
    ) {
        let q = &self.actors[i].body;
        let (centre, sector) = (q.pos, q.sector);
        out.push(CombatEvent::GroundPound {
            actor: i,
            point: centre,
            radius: plan.radius,
        });
        let dealt = |b: &Body| pound_damage(map, centre, sector, plan, b);
        if player.vitals.health.alive()
            && let Some(n) = dealt(player.body)
        {
            hurt_player(player, P0, n, self.damage_scale, centre, out);
        }
        let hits: Vec<(usize, i32)> = (0..self.actors.len())
            .filter(|&j| j != i && self.actors[j].targetable())
            .filter_map(|j| dealt(&self.actors[j].body).map(|n| (j, n)))
            .collect();
        for (j, n) in hits {
            out.extend(self.damage_actor(defs, j, n, Shooter::Actor(i)));
        }
    }
}

/// The damage a pound at `centre` deals to `b`, if it reaches it (see `ground_pound`).
pub fn pound_damage(
    map: &Map,
    centre: Vec3,
    sector: SectorId,
    plan: PoundDef,
    b: &Body,
) -> Option<i32> {
    if !b.on_ground || (b.pos.z - centre.z).abs() > POUND_REACH_Z {
        return None;
    }
    let d = b.pos.truncate().distance(centre.truncate());
    if d > plan.radius {
        return None;
    }
    let lift = Vec3::Z * LOW_TRACE;
    if !can_see(map, centre + lift, sector, b.pos + lift) {
        return None;
    }
    let n = (plan.damage as f32 * (1.0 - d / plan.radius)).round() as i32;
    (n > 0).then_some(n)
}

/// Whether a body of `radius` and `height` fits at `at`: inside a still sector tall enough,
/// within a step of `from`'s floor, clear of every blocking wall and of `bodies`, and in sight
/// of `from` at knee height.
fn spot_free(
    map: &Map,
    from: Vec3,
    sector: SectorId,
    at: Vec2,
    radius: f32,
    height: f32,
    bodies: &[Body],
) -> bool {
    let Some(s) = map.find_sector(at, Some(sector)) else {
        return false;
    };
    let sec = &map.sectors[s];
    if sec.mover.is_some()
        || sec.hazard.is_some()
        || sec.ceil_z - sec.floor_z < height
        || (sec.floor_z - from.z).abs() > Tuning::default().step_height
    {
        return false;
    }
    let near_wall = map.walls.iter().any(|w| {
        w.passage().is_none() && closest_point_on_segment(at, w.a, w.b).distance(at) < radius
    });
    let crowded = bodies
        .iter()
        .any(|b| b.pos.truncate().distance(at) < b.radius + radius);
    let lift = Vec3::Z * LOW_TRACE;
    !near_wall && !crowded && can_see(map, from + lift, sector, at.extend(sec.floor_z) + lift)
}
