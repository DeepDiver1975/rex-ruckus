//! The client's second clock: everything but its own player is drawn [`INTERP_DELAY`] ticks
//! behind the newest snapshot. Snapshots and the host's relayed events wait in [`Timeline`];
//! [`advance_timeline`] moves the render tick on by one each fixed tick, applies the snapshots
//! it reaches, releases each one's events right after it, and blends the poses between two
//! snapshots on the ticks in between.

use std::collections::VecDeque;

use bevy::prelude::*;
use rr_core::combat::{Combat, CombatEvent};
use rr_core::events::{combat_event_fits, mech_event_fits, weapon_event_fits};
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::Snapshot;
use rr_net::TickEvents;

use super::client::{NetClient, apply_players};
use super::predict::CORRECTION_SNAP;
use crate::breakables::put_out_lights;
use crate::combat::{FxQueue, GameDefs, LevelCombat};
use crate::flow::{LevelRuns, LocalSeat, settle_combat};
use crate::level::CurrentMap;
use crate::mechanics::{DirtySectors, LevelMechanics};
use crate::player::{LocalPlayer, Look, Player, PlayerBody, PrevFeet};

/// How far (ticks) the world is drawn behind the newest snapshot: 100 ms at 60 Hz, enough for
/// the next snapshot (every 2 ticks) to be in before the render tick reaches it.
pub const INTERP_DELAY: u32 = 6;

/// How far (ticks) the render tick may drift from `newest - INTERP_DELAY` before it jumps back.
pub const MAX_DRIFT: u32 = 6;

/// How far (ticks) the render tick may drift from `newest - INTERP_DELAY` before it eases back:
/// it holds a tick when this far ahead and steps two when this far behind.
pub const EASE_DRIFT: u32 = 2;

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
    /// Whether this run already reported a full hold (one warning per run).
    overflowed: bool,
    /// Whether no snapshot of this run was applied yet: the first one also brings in what
    /// happened before the client joined (see [`apply_snapshot`]).
    fresh: bool,
    /// What the first snapshot of this run synthesised (`ActorSpawned`, `LightBroken`), up to
    /// tick `covered_tick`: a relayed copy of one of these is the same news, dropped.
    pub covered: Vec<CombatEvent>,
    pub covered_tick: u32,
    /// How often the render tick jumped back onto the target after drifting past
    /// [`MAX_DRIFT`] (not counting the first placement).
    pub resyncs: u32,
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
            fresh: true,
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
            // The front is the blend base once it is applied: the oldest waiting one goes.
            let base = self.snaps.front().is_some_and(|(_, s)| s.tick <= applied);
            self.snaps.remove(usize::from(base));
            self.overflow("snapshots");
        }
        true
    }

    /// Reports, once per run, that the hold is full and the oldest go.
    fn overflow(&mut self, what: &str) {
        if !self.overflowed {
            self.overflowed = true;
            warn!("the co-op client holds too many {what}; dropping the oldest");
        }
    }

    /// Holds a tick's events unless they belong to the old run.
    pub fn push_events(&mut self, ev: TickEvents) {
        if ev.tick <= self.floor {
            return;
        }
        self.events.push_back(ev);
        while self.events.len() > MAX_HELD_EVENTS {
            self.events.pop_front();
            self.overflow("event messages");
        }
    }

    /// Moves the render tick on by one towards `target = newest - INTERP_DELAY`: by none when
    /// that would put it [`EASE_DRIFT`] or more ahead, by two when [`EASE_DRIFT`] or more
    /// behind, and straight onto the target when it has none yet or would drift more than
    /// [`MAX_DRIFT`]. `None` while nothing is held.
    pub fn step_render(&mut self) -> Option<u32> {
        let stepped = self.render.map(|r| r + 1);
        let Some(newest) = self.newest() else {
            self.render = stepped;
            return stepped;
        };
        let target = newest.saturating_sub(INTERP_DELAY);
        self.render = Some(match stepped {
            None => target,
            Some(r) if r.abs_diff(target) > MAX_DRIFT => {
                self.resyncs += 1;
                target
            }
            Some(r) if r >= target + EASE_DRIFT => r - 1,
            Some(r) if r + EASE_DRIFT <= target => r + 1,
            Some(r) => r,
        });
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
/// run). The first one of a run also shows what happened before the `Load` (a late joiner's
/// level is the authored one): the actors it appends get their visuals (`ActorSpawned`) and
/// the lights it breaks go dark without sparks, unless a held event of this run reports them.
/// Later ones leave that to the relayed events.
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
    if std::mem::take(&mut world.resource_mut::<Timeline>().fresh) {
        // What a held event of this run reports is the event's to show, with its effects.
        let timeline = world.resource::<Timeline>();
        let relayed = |want: &CombatEvent| {
            timeline
                .events
                .iter()
                .filter(|e| e.tick <= snap.tick)
                .any(|e| e.combat.contains(want))
        };
        let spawned: Vec<CombatEvent> = changes
            .actors_added
            .clone()
            .map(CombatEvent::ActorSpawned)
            .filter(|e| !relayed(e))
            .collect();
        let lights: Vec<usize> = changes
            .lights
            .iter()
            .copied()
            .filter(|&i| !relayed(&CombatEvent::LightBroken(i)))
            .collect();
        let mut timeline = world.resource_mut::<Timeline>();
        timeline.covered = spawned
            .iter()
            .cloned()
            .chain(lights.iter().copied().map(CombatEvent::LightBroken))
            .collect();
        timeline.covered_tick = snap.tick;
        world.resource_mut::<FxQueue>().combat.extend(spawned);
        put_out_lights(world, &lights);
    }
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
    let (covered, covered_tick) = {
        let t = world.resource::<Timeline>();
        (t.covered.clone(), t.covered_tick)
    };
    let (Some(combat), Some(mech)) = (
        world.get_resource::<LevelCombat>(),
        world.get_resource::<LevelMechanics>(),
    ) else {
        return;
    };
    let map = &world.resource::<CurrentMap>().0;
    let mut fx = FxQueue::default();
    for ev in due {
        // A late joiner's first snapshot already showed these (see `apply_snapshot`).
        let old = ev.tick <= covered_tick;
        fx.combat.extend(ev.combat.into_iter().filter(|e| {
            let shown = matches!(
                e,
                CombatEvent::ActorSpawned(_) | CombatEvent::LightBroken(_)
            ) && old
                && covered.contains(e);
            !shown && combat_event_fits(e, &combat.0, map)
        }));
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
    let mut ends = None;
    let players = world.resource_scope(|world, timeline: Mut<Timeline>| {
        let (Some((_, a)), Some((_, b))) = (timeline.snaps.front(), timeline.snaps.get(1)) else {
            return None;
        };
        if a.tick != last || b.tick <= a.tick {
            return None;
        }
        let f = render.saturating_sub(a.tick) as f32 / (b.tick - a.tick) as f32;
        let mut combat = world.get_resource_mut::<LevelCombat>()?;
        let players = a.blend(b, f, &mut combat.0);
        settle_unblended(a, b, &mut combat.0);
        ends = Some((a.players.clone(), b.players.clone()));
        Some(players)
    });
    let (Some(players), Some((a_players, b_players))) = (players, ends) else {
        return false;
    };
    // Only the feet and the facing are blended; the rest of a remote player is its last
    // snapshot's.
    let mut q = world.query_filtered::<
        (&Player, &mut PlayerBody, &mut PrevFeet, Option<&mut Look>),
        Without<LocalPlayer>,
    >();
    for (p, mut body, mut prev, look) in q.iter_mut(world) {
        match players.iter().find(|(id, ..)| *id == p.id) {
            Some(&(_, b, angle, pitch)) => {
                // A respawn jumps across the map between two snapshots: land there instead
                // of sliding, and rest the interpolation on it.
                let to = b_players.iter().find(|q| q.id == p.id).map(|q| q.body.pos);
                let from = a_players.iter().find(|q| q.id == p.id).map(|q| q.body.pos);
                let jumped = from.zip(to).is_some_and(|(f, t)| teleported(f, t));
                prev.0 = body.0.pos;
                body.0.pos = if jumped { to.unwrap_or(b.pos) } else { b.pos };
                if jumped {
                    prev.0 = body.0.pos;
                }
                if let Some(mut look) = look {
                    look.angle = angle;
                    look.pitch = pitch;
                }
            }
            // Not in both snapshots: it holds still rather than replay its last step.
            None if prev.0 != body.0.pos => prev.0 = body.0.pos,
            None => {}
        }
    }
    true
}

/// Whether a player moved too far between two snapshots to have walked it (a respawn): the
/// same distance at which the local player's correction snaps.
fn teleported(from: Vec3, to: Vec3) -> bool {
    from.distance(to) > CORRECTION_SNAP
}

/// Rests the actors and projectiles [`Snapshot::blend`] left alone (not carried by both `a`
/// and `b`) on their current pose, so frame interpolation does not replay their last step.
fn settle_unblended(a: &Snapshot, b: &Snapshot, combat: &mut Combat) {
    let both = a.actors.len().min(b.actors.len());
    for actor in combat.actors.iter_mut().skip(both) {
        actor.prev_pos = actor.body.pos;
    }
    let carried = |s: &Snapshot, id: u32| s.projectiles.iter().any(|p| p.id == id);
    for p in &mut combat.projectiles {
        if !(carried(a, p.id) && carried(b, p.id)) {
            p.prev = p.pos;
        }
    }
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
    // Through a system so `settle_combat` gets a `ResMut` and touches change detection only
    // when something moved.
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
    fn a_jump_of_more_than_a_metre_is_a_teleport() {
        let a = Vec3::new(2.0, 3.0, 0.0);
        assert!(!teleported(a, a + Vec3::new(0.9, 0.0, 0.0)));
        assert!(teleported(a, a + Vec3::new(0.0, 1.1, 0.0)));
        assert!(teleported(a, Vec3::new(30.0, -4.0, 1.0)));
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
    fn render_eases_back_onto_the_target() {
        let mut t = Timeline::default();
        t.push_snapshot(PlayerId(1), snap(20), 0);
        assert_eq!(t.step_render(), Some(14));
        // No newer snapshot: one tick ahead is fine, two is held.
        assert_eq!(t.step_render(), Some(15));
        assert_eq!(t.step_render(), Some(15));
        // Four ahead of it in one go: two at a time until within one.
        t.push_snapshot(PlayerId(1), snap(25), 0);
        assert_eq!(t.step_render(), Some(17));
        assert_eq!(t.step_render(), Some(18));
        assert_eq!(t.step_render(), Some(19));
        assert_eq!(t.resyncs, 0);
        // Past the drift limit it still jumps.
        t.push_snapshot(PlayerId(1), snap(40), 0);
        assert_eq!(t.step_render(), Some(34));
        assert_eq!(t.resyncs, 1);
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
    fn the_blend_base_is_never_evicted() {
        let mut t = Timeline::default();
        t.push_snapshot(PlayerId(1), snap(1), 0);
        for i in 2..=100 {
            t.push_snapshot(PlayerId(1), snap(i), 1);
        }
        assert_eq!(t.snaps.len(), MAX_HELD_SNAPS);
        assert_eq!(t.snaps[0].1.tick, 1, "the applied base stays");
        assert_eq!(t.snaps[1].1.tick, 100 - MAX_HELD_SNAPS as u32 + 2);
    }

    #[test]
    fn blending_rests_what_only_one_snapshot_carries() {
        use rr_core::defs::Defs;
        use rr_core::projectile::{Projectile, Shooter, Targets};
        let map = rr_core::fixtures::combat_room();
        let mut combat = Combat::spawn(&map, &Defs::builtin(), 1);
        let shot = |id: u32, x: f32| Projectile {
            id,
            pos: Vec3::new(x, 2.0, 1.0),
            prev: Vec3::new(x - 0.5, 2.0, 1.0),
            vel: Vec3::X,
            sector: 0,
            radius: 0.1,
            damage: 1,
            owner: Shooter::World,
            targets: Targets::All,
            life: 1.0,
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        };
        // 1 is in both snapshots, 2 only in the first (it hit something).
        combat.projectiles = vec![shot(1, 2.0), shot(2, 3.0)];
        let mut a = snap(10);
        a.projectiles = vec![shot(1, 2.0), shot(2, 3.0)];
        let mut b = snap(12);
        b.projectiles = vec![shot(1, 3.0)];
        let mut world = World::new();
        world.insert_resource(LevelCombat(combat));
        let mut t = Timeline::default();
        t.snaps.push_back((PlayerId(1), a));
        t.snaps.push_back((PlayerId(1), b));
        world.insert_resource(t);
        assert!(blend(&mut world, 10, 11));
        let c = &world.resource::<LevelCombat>().0;
        assert_eq!(c.projectiles[0].pos, Vec3::new(2.5, 2.0, 1.0));
        assert_eq!(c.projectiles[0].prev, Vec3::new(2.0, 2.0, 1.0));
        assert_eq!(
            c.projectiles[1].prev, c.projectiles[1].pos,
            "the lone shot rests"
        );
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
