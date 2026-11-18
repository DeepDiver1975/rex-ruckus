//! The client's second clock: everything but its own player is drawn [`INTERP_DELAY`] ticks
//! behind the newest snapshot. Snapshots and the host's relayed events wait in [`Timeline`];
//! [`advance_timeline`] moves the render tick on by one each fixed tick, applies the snapshots
//! it reaches, releases each one's events right after it, and blends the poses between two
//! snapshots on the ticks in between.

use std::collections::VecDeque;

use bevy::prelude::*;
use rr_core::events::{combat_event_fits, mech_event_fits, weapon_event_fits};
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::Snapshot;
use rr_net::TickEvents;

use super::client::{NetClient, apply_players};
use crate::combat::{FxQueue, GameDefs, LevelCombat};
use crate::flow::{LevelRuns, LocalSeat, settle_combat};
use crate::level::CurrentMap;
use crate::mechanics::{DirtySectors, LevelMechanics};
use crate::player::{LocalPlayer, Player, PlayerBody, PrevFeet};

/// How far (ticks) the world is drawn behind the newest snapshot: 100 ms at 60 Hz, enough for
/// the next snapshot (every 2 ticks) to be in before the render tick reaches it.
pub const INTERP_DELAY: u32 = 6;

/// How far (ticks) the render tick may drift from `newest - INTERP_DELAY` before it jumps back.
pub const MAX_DRIFT: u32 = 6;

/// Most snapshots [`Timeline`] holds; beyond, the oldest go.
pub const MAX_HELD_SNAPS: usize = 32;

/// Most event messages [`Timeline`] holds; beyond, the oldest go.
pub const MAX_HELD_EVENTS: usize = 256;

/// The snapshots and relayed events waiting for the render tick, oldest first. The front
/// snapshot is the last applied one (the pose blends start from) once one was applied.
#[derive(Resource, Default)]
pub struct Timeline {
    /// The host tick the world is drawn at; `None` until a snapshot of this run is in.
    pub render: Option<u32>,
    pub snaps: VecDeque<(PlayerId, Snapshot)>,
    pub events: VecDeque<TickEvents>,
    /// The tick of the last `Load`: anything at or before it is the old run's, dropped on
    /// arrival.
    pub floor: u32,
}

impl Timeline {
    /// The tick of the newest snapshot held.
    pub fn newest(&self) -> Option<u32> {
        self.snaps.back().map(|(_, s)| s.tick)
    }

    /// Forgets everything held for a run that starts after `floor` (a `Load`).
    pub fn reset(&mut self, floor: u32) {
        *self = Timeline {
            floor,
            ..Timeline::default()
        };
    }

    /// Holds `snap` if it is newer than the floor, `applied` and every snapshot held (UDP may
    /// reorder); true if it was.
    pub fn push_snapshot(&mut self, you: PlayerId, snap: Snapshot, applied: u32) -> bool {
        let newest = self.newest().unwrap_or(0).max(applied).max(self.floor);
        if snap.tick <= newest {
            return false;
        }
        self.snaps.push_back((you, snap));
        while self.snaps.len() > MAX_HELD_SNAPS {
            self.snaps.pop_front();
        }
        true
    }

    /// Holds a tick's events unless they belong to the old run.
    pub fn push_events(&mut self, ev: TickEvents) {
        if ev.tick <= self.floor {
            return;
        }
        self.events.push_back(ev);
        while self.events.len() > MAX_HELD_EVENTS {
            self.events.pop_front();
        }
    }

    /// Moves the render tick on by one, back onto `newest - INTERP_DELAY` when it has none yet
    /// or drifted more than [`MAX_DRIFT`] from it. `None` while nothing is held.
    pub fn step_render(&mut self) -> Option<u32> {
        let stepped = self.render.map(|r| r + 1);
        let Some(newest) = self.newest() else {
            self.render = stepped;
            return stepped;
        };
        let target = newest.saturating_sub(INTERP_DELAY);
        self.render = match stepped {
            Some(r) if r.abs_diff(target) <= MAX_DRIFT => Some(r),
            _ => Some(target),
        };
        self.render
    }

    /// Takes the held events of ticks up to `tick`, in arrival order.
    fn take_events(&mut self, tick: u32) -> Vec<TickEvents> {
        let (due, later): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|e| e.tick <= tick);
        self.events = later;
        due.into()
    }
}

/// The client's world tick (in place of a simulation step): see the module docs. The local
/// player is prediction's and is never moved here.
pub fn advance_timeline(world: &mut World) {
    let Some(applied) = world.resource::<NetClient>().last_tick else {
        freeze_remote(world);
        return;
    };
    let Some(render) = world.resource_mut::<Timeline>().step_render() else {
        freeze_remote(world);
        return;
    };
    let mut last = applied;
    let mut stepped = false;
    loop {
        let mut timeline = world.resource_mut::<Timeline>();
        let Some(i) = timeline.snaps.iter().position(|(_, s)| s.tick > last) else {
            break;
        };
        if timeline.snaps[i].1.tick > render {
            break;
        }
        let (you, snap) = timeline.snaps.remove(i).expect("found above");
        if !apply_snapshot(world, you, &snap) {
            continue;
        }
        last = snap.tick;
        stepped = true;
        release_events(world, snap.tick);
        let mut timeline = world.resource_mut::<Timeline>();
        timeline.snaps.drain(..i);
        timeline.snaps.push_front((you, snap));
    }
    if !stepped && !blend(world, last, render) {
        freeze_remote(world);
    }
}

/// Applies `snap` to the level and the players; false if it does not fit (reported once per
/// run).
fn apply_snapshot(world: &mut World, you: PlayerId, snap: &Snapshot) -> bool {
    if !world.contains_resource::<LevelCombat>() || !world.contains_resource::<LevelMechanics>() {
        return false;
    }
    let result = world.resource_scope(|world, mut combat: Mut<LevelCombat>| {
        world.resource_scope(|world, mut mech: Mut<LevelMechanics>| {
            world.resource_scope(|world, mut map: Mut<CurrentMap>| {
                let defs = &world.resource::<GameDefs>().0;
                snap.apply(&mut combat.0, &mut mech.0, &mut map.0, defs)
            })
        })
    });
    let changes = match result {
        Ok(changes) => changes,
        Err(e) => {
            let run = world.resource::<LevelRuns>().0;
            let mut net = world.resource_mut::<NetClient>();
            if net.warned_run != Some(run) {
                net.warned_run = Some(run);
                warn!("dropped a snapshot that does not fit the level: {e:?}");
            }
            return false;
        }
    };
    world.resource_mut::<NetClient>().last_tick = Some(snap.tick);
    // Shards, sounds and lights come with the relayed events; the meshes rebuild from here.
    let glass_sectors: Vec<usize> = {
        let map = &world.resource::<CurrentMap>().0;
        changes
            .glass
            .iter()
            .flat_map(|&w| std::iter::once(map.walls[w].sector).chain(map.walls[w].next_sector))
            .collect()
    };
    world
        .resource_mut::<DirtySectors>()
        .0
        .extend(changes.sectors.iter().copied().chain(glass_sectors));
    apply_players(world, you, &snap.players);
    true
}

/// Releases the held events of ticks up to `tick` into [`FxQueue`]: each must fit the
/// client's world, and the local player's weapon events are dropped (prediction played them).
fn release_events(world: &mut World, tick: u32) {
    let due = world.resource_mut::<Timeline>().take_events(tick);
    if due.is_empty() {
        return;
    }
    let local = world.get_resource::<LocalSeat>().map(|s| s.0);
    let (Some(combat), Some(mech)) = (
        world.get_resource::<LevelCombat>(),
        world.get_resource::<LevelMechanics>(),
    ) else {
        return;
    };
    let map = &world.resource::<CurrentMap>().0;
    let mut fx = FxQueue::default();
    for ev in due {
        fx.combat.extend(
            ev.combat
                .into_iter()
                .filter(|e| combat_event_fits(e, &combat.0, map)),
        );
        fx.weapon.extend(ev.weapon.into_iter().filter(|(p, e)| {
            Some(*p) != local && usize::from(p.0) < MAX_PLAYERS && weapon_event_fits(e)
        }));
        fx.mech.extend(
            ev.mech
                .into_iter()
                .filter(|e| mech_event_fits(e, &mech.0, map)),
        );
    }
    let mut queue = world.resource_mut::<FxQueue>();
    queue.combat.append(&mut fx.combat);
    queue.weapon.append(&mut fx.weapon);
    queue.mech.append(&mut fx.mech);
}

/// Draws the world at `render` between the last applied snapshot (tick `last`, the front of
/// the timeline) and the next one held; false if there is no next one.
fn blend(world: &mut World, last: u32, render: u32) -> bool {
    let players = world.resource_scope(|world, timeline: Mut<Timeline>| {
        let (Some((_, a)), Some((_, b))) = (timeline.snaps.front(), timeline.snaps.get(1)) else {
            return None;
        };
        if a.tick != last || b.tick <= a.tick {
            return None;
        }
        let f = render.saturating_sub(a.tick) as f32 / (b.tick - a.tick) as f32;
        let mut combat = world.get_resource_mut::<LevelCombat>()?;
        Some(a.blend(b, f, &mut combat.0))
    });
    let Some(players) = players else {
        return false;
    };
    // Only the feet are blended; the rest of a remote player is its last snapshot's.
    let mut q =
        world.query_filtered::<(&Player, &mut PlayerBody, &mut PrevFeet), Without<LocalPlayer>>();
    for (p, mut body, mut prev) in q.iter_mut(world) {
        if let Some((_, b)) = players.iter().find(|(id, _)| *id == p.id) {
            prev.0 = body.0.pos;
            body.0.pos = b.pos;
        }
    }
    true
}

/// Rests every interpolation endpoint but the local player's (prediction steps it) on the
/// current pose, so frames without a next snapshot hold still.
fn freeze_remote(world: &mut World) {
    let mut q = world.query_filtered::<(&PlayerBody, &mut PrevFeet), Without<LocalPlayer>>();
    for (body, mut prev) in q.iter_mut(world) {
        if prev.0 != body.0.pos {
            prev.0 = body.0.pos;
        }
    }
    if let Err(e) = world.run_system_cached(settle_remote_combat) {
        warn!("cannot settle interpolation: {e}");
    }
}

fn settle_remote_combat(combat: Option<ResMut<LevelCombat>>) {
    if let Some(mut combat) = combat {
        settle_combat(&mut combat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(tick: u32) -> Snapshot {
        Snapshot {
            tick,
            actors: vec![],
            projectiles: vec![],
            world: rr_core::snapshot::WorldSnap {
                movers: vec![],
                switch_on: vec![],
                taken: vec![],
                props: vec![],
                destruct: Default::default(),
                glass_broken: vec![],
                quakes: vec![],
            },
            players: vec![],
        }
    }

    #[test]
    fn render_starts_and_resyncs_behind_the_newest() {
        let mut t = Timeline::default();
        assert_eq!(t.step_render(), None);
        assert!(t.push_snapshot(PlayerId(1), snap(20), 0));
        assert_eq!(t.step_render(), Some(20 - INTERP_DELAY));
        assert_eq!(t.step_render(), Some(21 - INTERP_DELAY));
        assert!(t.push_snapshot(PlayerId(1), snap(60), 0));
        assert_eq!(t.step_render(), Some(60 - INTERP_DELAY));
    }

    #[test]
    fn old_and_reordered_arrivals_are_dropped() {
        let mut t = Timeline::default();
        t.reset(10);
        assert!(!t.push_snapshot(PlayerId(1), snap(10), 0));
        assert!(t.push_snapshot(PlayerId(1), snap(14), 0));
        assert!(!t.push_snapshot(PlayerId(1), snap(12), 0));
        assert!(!t.push_snapshot(PlayerId(1), snap(14), 0));
        let ev = |tick| TickEvents {
            tick,
            combat: vec![],
            weapon: vec![],
            mech: vec![],
        };
        t.push_events(ev(10));
        t.push_events(ev(11));
        assert_eq!(t.events.len(), 1);
    }

    #[test]
    fn holding_is_bounded() {
        let mut t = Timeline::default();
        for i in 1..=100 {
            t.push_snapshot(PlayerId(1), snap(i), 0);
            t.push_events(TickEvents {
                tick: i,
                combat: vec![],
                weapon: vec![],
                mech: vec![],
            });
        }
        for i in 0..300 {
            t.push_events(TickEvents {
                tick: 100 + i,
                combat: vec![],
                weapon: vec![],
                mech: vec![],
            });
        }
        assert_eq!(t.snaps.len(), MAX_HELD_SNAPS);
        assert_eq!(
            t.snaps.front().unwrap().1.tick,
            100 - MAX_HELD_SNAPS as u32 + 1
        );
        assert_eq!(t.events.len(), MAX_HELD_EVENTS);
    }
}
