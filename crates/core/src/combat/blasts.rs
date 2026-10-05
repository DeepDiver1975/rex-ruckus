//! The explosion queue: rockets, detonated bombs and barrels all go off through
//! [`Combat::process_blasts`], one tick at a time.

use super::{BODY_PLAYER, Combat, CombatEvent, PlayerTarget, hurt_player};
use crate::collide::Body;
use crate::defs::Defs;
use crate::explosion::{Blast, solve};
use crate::map::Map;
use glam::Vec3;

/// Gap left between a blast centre and the surface it went off against (m).
pub const BLAST_NUDGE: f32 = 0.05;
/// How far (m) the bang of a blast carries: sleepers this close, in sectors sound reaches,
/// wake up.
pub const BLAST_NOISE: f32 = 40.0;

/// A blast waiting to go off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingBlast {
    /// Seconds until it goes off; 0 means this tick.
    pub fuse: f32,
    pub blast: Blast,
}

impl Combat {
    /// Queues `blast` to go off in `fuse` seconds.
    pub(super) fn queue_blast(&mut self, fuse: f32, blast: Blast) {
        self.pending_blasts.push(PendingBlast { fuse, blast });
    }

    /// Runs every queued blast's fuse down by `dt` and sets off, in queue order, those that
    /// burn out. Blasts queued while these go off (a barrel's death blast, always on a fuse)
    /// wait for a later tick, so a chain advances one link per fuse and always ends: each
    /// barrel dies, and queues its blast, at most once.
    pub(super) fn process_blasts(
        &mut self,
        map: &Map,
        defs: &Defs,
        player: &mut PlayerTarget,
        dt: f32,
        out: &mut Vec<CombatEvent>,
    ) {
        for pb in &mut self.pending_blasts {
            pb.fuse -= dt;
        }
        let (due, waiting) = std::mem::take(&mut self.pending_blasts)
            .into_iter()
            .partition(|pb| pb.fuse <= 0.0);
        self.pending_blasts = waiting;
        for pb in due {
            self.apply_blast(map, defs, player, &pb, out);
        }
    }

    /// One explosion going off: reports `Explosion`, deals splash once to each body it reaches
    /// (a body a projectile hit directly included): the player through `Vitals` (reporting
    /// `PlayerHurt { from: centre }`), living actors through the usual hurt path, so pain,
    /// kills and barrel chains behave as for any other damage. Then its noise
    /// (`BLAST_NOISE`) wakes sleepers.
    ///
    /// World effects of a blast (glass, cracked walls, lights) hook in here.
    fn apply_blast(
        &mut self,
        map: &Map,
        defs: &Defs,
        player: &mut PlayerTarget,
        pb: &PendingBlast,
        out: &mut Vec<CombatEvent>,
    ) {
        let blast = &pb.blast;
        out.push(CombatEvent::Explosion {
            point: blast.center,
            radius: blast.splash.radius,
        });
        let mut bodies: Vec<Body> = vec![*player.body];
        bodies.extend(self.actors.iter().map(|a| a.body));
        let skip = |i: usize| {
            if i == BODY_PLAYER {
                !player.vitals.health.alive()
            } else {
                !self.actors[i - 1].alive()
            }
        };
        for hit in solve(map, blast, &bodies, skip) {
            if hit.body == BODY_PLAYER {
                hurt_player(player, hit.damage, blast.center, out);
            } else {
                out.extend(self.damage_actor(defs, hit.body - 1, hit.damage));
            }
        }
        let woken = self.make_noise(map, blast.center, blast.sector, BLAST_NOISE);
        out.extend(woken.into_iter().map(CombatEvent::ActorWoke));
    }
}

/// Sector of `p`, searched from `hint` (which is returned if `p` is outside the map).
pub(super) fn sector_of(map: &Map, p: Vec3, hint: usize) -> usize {
    map.find_sector(p.truncate(), Some(hint)).unwrap_or(hint)
}
