//! Moving sectors (doors, lifts and sliders), switches, the channel bus and keycard pickups.
//! Like Build's `setanimation`/`doanimations`, each mover slides one plane of its sector (a door's
//! ceiling, a lift's floor) between two heights, and carries whoever stands on it. A slider moves
//! the whole sector sideways instead, like Build's sliding sectors.

use crate::collide::{Body, clip_move, touches_sector, z_range};
use crate::geom::closest_point_on_segment;
use crate::health::PLAYER_MAX_HEALTH;
use crate::map::{
    Channel, ItemKind, Key, KeySet, Map, MoverDef, MoverKind, RawBlast, RawQuake, SectorId,
    SwitchAction, WallId,
};
use crate::players::PlayerId;
use crate::props::{PropKind, PropOutcome};
use crate::vitals::Vitals;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Feet within this distance of a lift floor ride along with it.
const CARRY_EPS: f32 = 0.01;
/// Horizontal reach, beyond the body radius, for picking items up.
pub const PICKUP_REACH: f32 = 0.6;
/// Items more than this far above or below the feet stay put.
const PICKUP_HEIGHT: f32 = 1.0;
/// Fraction of a quake's duration over which it fades out.
const QUAKE_FADE: f32 = 0.3;
/// Step height a slider's riders move with (the player's).
const RIDE_STEP: f32 = 0.55;
/// How far a slider may cut into a body beside or under it before it backs off.
const BLOCK_EPS: f32 = 1e-3;

/// Strength multiplier of a quake `elapsed` seconds into `duration`: 1, then linear to 0 over
/// the last `QUAKE_FADE` of it.
pub fn quake_fade(elapsed: f32, duration: f32) -> f32 {
    let left = duration - elapsed;
    let fade = QUAKE_FADE * duration;
    if left >= fade {
        1.0
    } else {
        (left / fade).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Motion {
    AtStart,
    ToEnd,
    AtEnd { wait: f32 },
    ToStart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub sector: SectorId,
    pub def: MoverDef,
    /// Height of the moving plane at the start pose (door: closed; lift: authored floor). A
    /// slider's `start`, `end` and `z` are distances along its path: 0, `|by|` and travelled.
    pub start: f32,
    pub end: f32,
    pub z: f32,
    pub motion: Motion,
    /// A slider's walls and their track-side twins, with their authored endpoints (empty for
    /// other kinds). Poses are set from these, so the slider never drifts.
    pub home: Vec<(WallId, Vec2, Vec2)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseTarget {
    Mover(usize),
    Switch(usize),
    /// Index into `map.props`; used through `use_prop`.
    Prop(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseOutcome {
    Activated,
    NeedKey(Key),
    Exit,
}

/// Something audible that happened in the mechanics, queued for `Mechanics::drain_events`.
///
/// `MoverStarted` fires when a mover leaves rest (`AtStart` or `AtEnd`), however it was
/// triggered: toggle, channel, use or an auto-close. `MoverStopped` fires when it arrives. A
/// reversal mid-motion (a re-toggle, or a body in the way) emits nothing: it is still moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MechEvent {
    MoverStarted {
        sector: SectorId,
        kind: MoverKind,
    },
    MoverStopped {
        sector: SectorId,
        kind: MoverKind,
    },
    /// Index into `map.switches`.
    SwitchUsed(usize),
    NeedKey(Key),
    Exit,
    /// `item` indexes `map.items`.
    ItemTaken {
        item: usize,
        kind: ItemKind,
    },
    /// Index into `map.triggers`; follows the trigger's own action.
    TriggerFired(usize),
    /// A quake on a fired channel began.
    QuakeStarted {
        strength: f32,
        duration: f32,
    },
    /// Prop `prop` (index into `map.props`) was used; never queued for `Busy`.
    PropUsed {
        prop: usize,
        kind: PropKind,
        outcome: PropOutcome,
    },
}

/// Health a toilet gives per use.
pub const TOILET_HEAL: i32 = 10;
/// Health a soda gives.
pub const SODA_HEAL: i32 = 5;
/// Seconds before a toilet can be flushed again.
pub const FLUSH_COOLDOWN: f32 = 1.5;
/// Seconds an arcade round (its jingle) takes before the cabinet takes another coin.
pub const ARCADE_COOLDOWN: f32 = 3.0;
/// Length of the jukebox stinger in seconds (`jukebox_stinger` in `assets/sounds/synth.ron`):
/// the jukebox is busy, and the level music ducked, for this long.
pub const JUKEBOX_SECS: f32 = 4.0;
/// Seconds before a payphone can be picked up again.
pub const PAYPHONE_COOLDOWN: f32 = 1.5;

/// A prop's runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropState {
    pub kind: PropKind,
    pub stock: u32,
    pub cooldown: f32,
    /// A payphone already gave its tip.
    pub tipped: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mechanics {
    pub movers: Vec<Mover>,
    /// Per switch: flips on each use (drives its lamp).
    pub switch_on: Vec<bool>,
    /// Per item: already picked up.
    pub taken: Vec<bool>,
    /// Per prop: stock and toilet cooldown.
    pub props: Vec<PropState>,
    /// Per trigger: a `once` trigger that already ran.
    trig_done: Vec<bool>,
    /// The sector `enter` last saw the player in (`None` before the first call).
    /// Per player id: the sector it was in at its last `enter` call.
    inside: Vec<Option<SectorId>>,
    /// Each authored quake with its elapsed time while it runs.
    quakes: Vec<(RawQuake, Option<f32>)>,
    /// Each authored blast, and whether its channel already set it off (blasts go off once).
    blasts: Vec<(RawBlast, bool)>,
    /// Blasts set off since `take_blasts` last ran, in map order; combat queues them.
    blasts_due: Vec<RawBlast>,
    mover_of: Vec<Option<usize>>,
    events: Vec<MechEvent>,
}

/// The lock keys of every mover listening on `ch`, in sector (= mover) order.
fn channel_locks(map: &Map, ch: Channel) -> impl Iterator<Item = Key> + '_ {
    map.sectors
        .iter()
        .filter_map(|s| s.mover)
        .filter(move |m| m.channel == Some(ch))
        .filter_map(|m| m.lock)
}

/// Every key a switch on channel `ch` needs: the lock of each mover listening on it. The one
/// rule shared by runtime (`Mechanics::activate`) and level validation.
pub fn channel_required_keys(map: &Map, ch: Channel) -> KeySet {
    let mut keys = KeySet::default();
    for k in channel_locks(map, ch) {
        keys.insert(k);
    }
    keys
}

impl Mechanics {
    /// Builds the runtime state and puts every sector into its start pose (doors closed).
    pub fn new(map: &mut Map) -> Mechanics {
        let mut movers = Vec::new();
        let mut mover_of = vec![None; map.sectors.len()];
        for (s, sector) in map.sectors.iter().enumerate() {
            let Some(def) = sector.mover else { continue };
            let (start, end) = match def.kind {
                MoverKind::Door | MoverKind::Crack => (sector.floor_z, sector.ceil_z),
                MoverKind::Lift { to } => (sector.floor_z, to),
                MoverKind::Slide { by } => (0.0, Vec2::from(by).length()),
            };
            let home = match def.kind {
                MoverKind::Slide { .. } => sector
                    .walls()
                    .flat_map(|w| std::iter::once(w).chain(map.walls[w].next_wall))
                    .map(|w| (w, map.walls[w].a, map.walls[w].b))
                    .collect(),
                _ => Vec::new(),
            };
            mover_of[s] = Some(movers.len());
            movers.push(Mover {
                sector: s,
                def,
                start,
                end,
                z: start,
                motion: Motion::AtStart,
                home,
            });
        }
        for mv in &movers {
            if matches!(mv.def.kind, MoverKind::Door | MoverKind::Crack) {
                map.sectors[mv.sector].ceil_z = mv.start;
            }
        }
        Mechanics {
            movers,
            switch_on: vec![false; map.switches.len()],
            taken: vec![false; map.items.len()],
            props: map
                .props
                .iter()
                .map(|p| PropState {
                    kind: p.kind,
                    stock: p.stock,
                    cooldown: 0.0,
                    tipped: false,
                })
                .collect(),
            trig_done: vec![false; map.triggers.len()],
            inside: Vec::new(),
            quakes: map.quakes.iter().map(|&q| (q, None)).collect(),
            blasts: map.blasts.iter().map(|&b| (b, false)).collect(),
            blasts_due: Vec::new(),
            mover_of,
            events: Vec::new(),
        }
    }

    /// Takes the events queued since the last call, oldest first.
    pub fn drain_events(&mut self) -> Vec<MechEvent> {
        std::mem::take(&mut self.events)
    }

    /// Queues a start event for mover `m` (for code that sets its motion directly).
    pub(crate) fn emit_started(&mut self, m: usize) {
        let mv = &self.movers[m];
        self.events.push(MechEvent::MoverStarted {
            sector: mv.sector,
            kind: mv.def.kind,
        });
    }

    pub fn mover_in(&self, s: SectorId) -> Option<usize> {
        self.mover_of.get(s).copied().flatten()
    }

    /// Sends a mover towards its other end, or reverses it mid-travel. A crack wall ignores
    /// this: only `Destruct::open_crack` moves it.
    pub fn toggle(&mut self, m: usize) {
        let mv = &mut self.movers[m];
        // A crack wall only opens to a blast; a one-shot mover only ever leaves its start once.
        if mv.def.kind == MoverKind::Crack || (mv.def.one_shot && mv.motion != Motion::AtStart) {
            return;
        }
        let at_rest = matches!(mv.motion, Motion::AtStart | Motion::AtEnd { .. });
        mv.motion = match mv.motion {
            Motion::AtStart | Motion::ToStart => Motion::ToEnd,
            Motion::AtEnd { .. } | Motion::ToEnd => Motion::ToStart,
        };
        if at_rest {
            self.emit_started(m);
        }
    }

    /// The channel bus: toggles every mover listening on `ch`, starts its quakes and sets off
    /// its blasts that have not gone off yet (handed to combat through `take_blasts`). Returns
    /// how many movers reacted.
    pub fn fire(&mut self, ch: Channel) -> usize {
        for (b, done) in &mut self.blasts {
            if b.channel == ch && !*done {
                *done = true;
                self.blasts_due.push(*b);
            }
        }
        for (q, t) in &mut self.quakes {
            if q.channel == ch {
                *t = Some(0.0);
                self.events.push(MechEvent::QuakeStarted {
                    strength: q.strength,
                    duration: q.duration,
                });
            }
        }
        let hits: Vec<usize> = (0..self.movers.len())
            .filter(|&m| self.movers[m].def.channel == Some(ch))
            .filter(|&m| self.movers[m].def.kind != MoverKind::Crack)
            .collect();
        for &m in &hits {
            self.toggle(m);
        }
        hits.len()
    }

    /// Takes the scripted blasts set off since the last call (`Combat::tick` queues them on
    /// their `delay`).
    pub fn take_blasts(&mut self) -> Vec<RawBlast> {
        std::mem::take(&mut self.blasts_due)
    }

    /// Runs an action that does not come from a switch (a trigger or a death): fires the
    /// channel, or queues `Exit` and returns true.
    pub fn apply(&mut self, action: SwitchAction) -> bool {
        match action {
            SwitchAction::Channel(ch) => {
                self.fire(ch);
                false
            }
            SwitchAction::Exit => {
                self.events.push(MechEvent::Exit);
                true
            }
        }
    }

    /// Player `player` is in `sector` this tick. On entering it (a different sector from that
    /// player's last call, or its first call; any player's entry counts), each of its triggers
    /// that may still run applies its action and then reports `TriggerFired(i)`. Being carried
    /// by a lift never changes the sector, so it never re-fires. Returns true when an exit
    /// trigger ran.
    pub fn enter(&mut self, map: &Map, player: PlayerId, sector: SectorId) -> bool {
        let p = player.0 as usize;
        if self.inside.len() <= p {
            self.inside.resize(p + 1, None);
        }
        if self.inside[p] == Some(sector) {
            return false;
        }
        self.inside[p] = Some(sector);
        let mut exit = false;
        for (i, t) in map.triggers.iter().enumerate() {
            if t.sector != sector || self.trig_done[i] {
                continue;
            }
            self.trig_done[i] = t.once;
            exit |= self.apply(t.action);
            self.events.push(MechEvent::TriggerFired(i));
        }
        exit
    }

    /// Elapsed seconds of each authored quake while it runs (map order), for snapshots.
    pub(crate) fn quake_times(&self) -> Vec<Option<f32>> {
        self.quakes.iter().map(|&(_, t)| t).collect()
    }

    /// How many quakes the level authors.
    pub(crate) fn quake_count(&self) -> usize {
        self.quakes.len()
    }

    /// Sets each quake's elapsed time from a snapshot; `times` must hold one per quake.
    pub(crate) fn set_quake_times(&mut self, times: &[Option<f32>]) {
        debug_assert_eq!(times.len(), self.quakes.len());
        for ((_, t), &s) in self.quakes.iter_mut().zip(times) {
            *t = s;
        }
    }

    /// Current screen-shake strength in 0..=1: the strongest running quake, faded.
    pub fn quake_strength(&self) -> f32 {
        self.quakes
            .iter()
            .filter_map(|(q, t)| t.map(|e| q.strength * quake_fade(e, q.duration)))
            .fold(0.0, f32::max)
    }

    /// The first lock key (in mover order) that `keys` lacks among the movers listening on `ch`.
    fn missing_listener_key(&self, map: &Map, ch: Channel, keys: KeySet) -> Option<Key> {
        channel_locks(map, ch).find(|&k| !keys.contains(k))
    }

    /// The player uses prop `i`; `secrets_left` is how many secrets the level still hides
    /// (`Destruct::secrets_left`). Toilet: +10 health up to 100 (a plain flush at full health),
    /// then a 1.5 s cooldown. Vending machine: +5 up to 100 per soda while stock lasts. Pool
    /// table: flavour. Arcade: a round, then `ARCADE_COOLDOWN`. Jukebox: a stinger, busy for
    /// `JUKEBOX_SECS`. Payphone: the first call gives `Tip(secrets_left)`, later ones a dial
    /// tone, each followed by `PAYPHONE_COOLDOWN`. Queues `PropUsed` unless the prop is busy
    /// (a cooldown running).
    pub fn use_prop(&mut self, i: usize, vitals: &mut Vitals, secrets_left: u32) -> PropOutcome {
        let p = &mut self.props[i];
        let before = vitals.health.hp;
        if p.cooldown > 0.0 {
            return PropOutcome::Busy;
        }
        let outcome = match p.kind {
            PropKind::Toilet => {
                p.cooldown = FLUSH_COOLDOWN;
                vitals.heal_to(TOILET_HEAL, PLAYER_MAX_HEALTH);
                match vitals.health.hp - before {
                    0 => PropOutcome::Flushed,
                    n => PropOutcome::Healed(n),
                }
            }
            PropKind::Vending if p.stock == 0 => PropOutcome::SoldOut,
            PropKind::Vending => {
                p.stock -= 1;
                vitals.heal_to(SODA_HEAL, PLAYER_MAX_HEALTH);
                PropOutcome::Dispensed(vitals.health.hp - before)
            }
            PropKind::PoolTable => PropOutcome::Racked,
            PropKind::Arcade => {
                p.cooldown = ARCADE_COOLDOWN;
                PropOutcome::Played
            }
            PropKind::Jukebox => {
                p.cooldown = JUKEBOX_SECS;
                PropOutcome::Stinger
            }
            PropKind::Payphone => {
                p.cooldown = PAYPHONE_COOLDOWN;
                if std::mem::replace(&mut p.tipped, true) {
                    PropOutcome::DialTone
                } else {
                    PropOutcome::Tip(secrets_left)
                }
            }
        };
        let kind = self.props[i].kind;
        self.events.push(MechEvent::PropUsed {
            prop: i,
            kind,
            outcome,
        });
        outcome
    }

    /// Pressing use on `target` while holding `keys`.
    pub fn activate(&mut self, map: &Map, target: UseTarget, keys: KeySet) -> UseOutcome {
        match target {
            // Props go through `use_prop` (they need the player's vitals).
            UseTarget::Prop(_) => UseOutcome::Activated,
            UseTarget::Mover(m) => {
                if let Some(k) = self.movers[m].def.lock
                    && !keys.contains(k)
                {
                    self.events.push(MechEvent::NeedKey(k));
                    return UseOutcome::NeedKey(k);
                }
                self.toggle(m);
                UseOutcome::Activated
            }
            UseTarget::Switch(i) => {
                let sw = map.switches[i];
                if let Some(k) = sw.key
                    && !keys.contains(k)
                {
                    self.events.push(MechEvent::NeedKey(k));
                    return UseOutcome::NeedKey(k);
                }
                if let SwitchAction::Channel(ch) = sw.action
                    && let Some(k) = self.missing_listener_key(map, ch, keys)
                {
                    self.events.push(MechEvent::NeedKey(k));
                    return UseOutcome::NeedKey(k);
                }
                self.switch_on[i] = !self.switch_on[i];
                self.events.push(MechEvent::SwitchUsed(i));
                match sw.action {
                    SwitchAction::Channel(ch) => {
                        self.fire(ch);
                        UseOutcome::Activated
                    }
                    SwitchAction::Exit => {
                        self.events.push(MechEvent::Exit);
                        UseOutcome::Exit
                    }
                }
            }
        }
    }

    /// Advances every mover by `dt`. Bodies standing on a moving floor ride along. A mover that
    /// would squeeze a body against a ceiling reverses instead. Returns the sectors whose heights changed.
    pub fn tick(&mut self, map: &mut Map, bodies: &mut [Body], dt: f32) -> Vec<SectorId> {
        let mut changed = Vec::new();
        for p in &mut self.props {
            p.cooldown = (p.cooldown - dt).max(0.0);
        }
        for (q, t) in &mut self.quakes {
            if let Some(e) = t {
                *e += dt;
                if *e >= q.duration {
                    *t = None;
                }
            }
        }
        for mv in &mut self.movers {
            let goal = match &mut mv.motion {
                Motion::AtStart => continue,
                Motion::AtEnd { wait } => {
                    if let Some(limit) = mv.def.auto_return {
                        *wait += dt;
                        if *wait >= limit {
                            mv.motion = Motion::ToStart;
                            self.events.push(MechEvent::MoverStarted {
                                sector: mv.sector,
                                kind: mv.def.kind,
                            });
                        }
                    }
                    continue;
                }
                Motion::ToEnd => mv.end,
                Motion::ToStart => mv.start,
            };
            let step = mv.def.speed * dt;
            let z = if (goal - mv.z).abs() <= step {
                goal
            } else {
                mv.z + step * (goal - mv.z).signum()
            };
            if set_plane(map, mv, z, bodies) {
                mv.z = z;
                changed.push(moved_sector(mv));
                if z == goal {
                    mv.motion = if goal == mv.end {
                        Motion::AtEnd { wait: 0.0 }
                    } else {
                        Motion::AtStart
                    };
                    self.events.push(MechEvent::MoverStopped {
                        sector: mv.sector,
                        kind: mv.def.kind,
                    });
                }
            } else {
                // Like a Build door hitting the player: go back the way we came.
                mv.motion = if goal == mv.end {
                    Motion::ToStart
                } else {
                    Motion::ToEnd
                };
            }
        }
        changed
    }

    /// Picks up every item within reach of `body` that `accept` takes. Returns the indices taken
    /// by this call; a refused item stays in the world for a later try.
    pub fn pickup(
        &mut self,
        map: &Map,
        body: &Body,
        mut accept: impl FnMut(ItemKind) -> bool,
    ) -> Vec<usize> {
        let mut got = Vec::new();
        for (i, item) in map.items.iter().enumerate() {
            if self.taken[i] || item.pos.distance(body.pos.truncate()) > body.radius + PICKUP_REACH
            {
                continue;
            }
            let floor = map
                .find_sector(item.pos, Some(body.sector))
                .map(|s| map.sectors[s].floor_z);
            if floor.is_some_and(|f| (body.pos.z - f).abs() <= PICKUP_HEIGHT) && accept(item.kind) {
                self.taken[i] = true;
                self.events.push(MechEvent::ItemTaken {
                    item: i,
                    kind: item.kind,
                });
                got.push(i);
            }
        }
        got
    }
}

/// How far a body's head pokes into the lowest ceiling over its footprint (≤ 0 when clear).
fn squeeze(map: &Map, b: &Body) -> f32 {
    let (_, ceil) = z_range(map, b.pos.truncate(), b.radius, b.sector);
    b.pos.z + b.height - ceil
}

/// Moves the mover's plane to `z`, carrying bodies on a lift floor. If any body touching the
/// sector would end up squeezed more than before, undoes everything and returns false.
fn set_plane(map: &mut Map, mv: &Mover, z: f32, bodies: &mut [Body]) -> bool {
    if let MoverKind::Slide { by } = mv.def.kind {
        return slide(map, mv, Vec2::from(by).normalize(), z, bodies);
    }
    let s = mv.sector;
    let touching: Vec<usize> = (0..bodies.len())
        .filter(|&i| touches_sector(map, &bodies[i], s))
        .collect();
    let before: Vec<f32> = touching.iter().map(|&i| squeeze(map, &bodies[i])).collect();
    let saved = bodies.to_vec();
    let (old_floor, old_ceil) = (map.sectors[s].floor_z, map.sectors[s].ceil_z);
    place_mover(map, mv, z);
    if let MoverKind::Lift { .. } = mv.def.kind {
        for &i in &touching {
            let b = &mut bodies[i];
            let riding = b.on_ground && (b.pos.z - old_floor).abs() < CARRY_EPS;
            // A rising floor also scoops up a body that was just above it (mid-jump).
            let overrun = b.pos.z < z && b.pos.z > old_floor - CARRY_EPS;
            if riding {
                // Land on the highest floor under the footprint: a rider overhanging a
                // ledge stays on it while the lift sinks away.
                b.pos.z = z_range(map, b.pos.truncate(), b.radius, b.sector).0;
            } else if overrun {
                b.pos.z = z;
            }
        }
    }
    let worse = touching
        .iter()
        .zip(&before)
        .any(|(&i, &was)| squeeze(map, &bodies[i]) > was.max(0.0) + 1e-4);
    if worse {
        map.sectors[s].floor_z = old_floor;
        map.sectors[s].ceil_z = old_ceil;
        bodies.copy_from_slice(&saved);
    }
    !worse
}

/// The sector to report as changed when mover `mv` moves (`Mechanics::tick` and a snapshot
/// apply share it): its own. A slider's track is its neighbour, so the renderer's rebuild of a
/// changed sector and its neighbours covers it.
pub(crate) fn moved_sector(mv: &Mover) -> SectorId {
    mv.sector
}

/// Writes mover `mv`'s pose at `z` into the map: a door's or crack's ceiling, a lift's floor, or
/// a slider's walls `z` metres down its path. Moves no bodies (`Mechanics::tick` carries them;
/// a snapshot apply only poses the map).
pub(crate) fn place_mover(map: &mut Map, mv: &Mover, z: f32) {
    match mv.def.kind {
        MoverKind::Door | MoverKind::Crack => map.sectors[mv.sector].ceil_z = z,
        MoverKind::Lift { .. } => map.sectors[mv.sector].floor_z = z,
        MoverKind::Slide { by } => place_slider(map, mv, Vec2::from(by).normalize(), z),
    }
}

/// Puts slider `mv` (and its twins in the track) `along` metres down its path `dir`.
fn place_slider(map: &mut Map, mv: &Mover, dir: Vec2, along: f32) {
    let off = dir * along;
    for &(w, a, b) in &mv.home {
        map.walls[w].a = a + off;
        map.walls[w].b = b + off;
    }
}

/// How far body `b` cuts into slider `s` (> 0 when they overlap), counting only a body that
/// reaches below the slider's floor or above its ceiling: one standing on it or flying over it
/// never blocks it.
fn cut(map: &Map, s: SectorId, b: &Body) -> f32 {
    let sec = &map.sectors[s];
    if b.pos.z >= sec.floor_z - CARRY_EPS && b.pos.z + b.height <= sec.ceil_z + CARRY_EPS {
        return f32::MIN;
    }
    let p = b.pos.truncate();
    let d = sec
        .walls()
        .map(|w| closest_point_on_segment(p, map.walls[w].a, map.walls[w].b).distance(p))
        .fold(f32::MAX, f32::min);
    if map.sector_contains(s, p) {
        b.radius + d
    } else {
        b.radius - d
    }
}

/// Moves slider `mv` to `z` along `dir`, carrying the bodies standing on it: each is moved by
/// the same offset with `clip_move` in the moved map, so a rider overhanging the edge is stopped
/// by the track's walls instead of being pushed into them. If the slider would cut deeper into
/// a body beside or under it, or squeeze one against a ceiling, undoes everything and returns
/// false (it then reverses, like a door hitting the player).
fn slide(map: &mut Map, mv: &Mover, dir: Vec2, z: f32, bodies: &mut [Body]) -> bool {
    let s = mv.sector;
    let floor = map.sectors[s].floor_z;
    let riders: Vec<bool> = bodies
        .iter()
        .map(|b| {
            b.on_ground
                && (b.pos.z - floor).abs() < CARRY_EPS
                && (b.sector == s || map.sector_contains(s, b.pos.truncate()))
        })
        .collect();
    let cut_before: Vec<f32> = bodies.iter().map(|b| cut(map, s, b)).collect();
    let squeeze_before: Vec<f32> = bodies.iter().map(|b| squeeze(map, b)).collect();
    let saved = bodies.to_vec();
    place_mover(map, mv, z);
    let delta = dir * (z - mv.z);
    for (b, &ride) in bodies.iter_mut().zip(&riders) {
        if ride {
            clip_move(map, b, delta, RIDE_STEP);
        } else {
            // The slider may have moved under (or out from under) a body it does not carry.
            if let Some(n) = map.find_sector(b.pos.truncate(), Some(b.sector)) {
                b.sector = n;
            }
            // Like a rising lift, it scoops up a body just below its floor (stepping off its
            // leading edge) instead of leaving its feet inside it.
            if b.pos.z < floor && b.pos.z > floor - CARRY_EPS && touches_sector(map, b, s) {
                b.pos.z = floor;
            }
        }
    }
    let worse = (0..bodies.len()).any(|i| {
        (!riders[i] && cut(map, s, &bodies[i]) > cut_before[i].max(0.0) + BLOCK_EPS)
            || squeeze(map, &bodies[i]) > squeeze_before[i].max(0.0) + 1e-4
    });
    if worse {
        place_mover(map, mv, mv.z);
        bodies.copy_from_slice(&saved);
    }
    !worse
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::clip_move;
    use crate::fixtures::{door_rooms, engine_room, lift_shaft, slide_yard};
    use crate::props::{PropKind, PropOutcome};
    use crate::vitals::Vitals;
    use glam::Vec2;

    const DT: f32 = 1.0 / 60.0;

    fn setup(map: Map) -> (Map, Mechanics) {
        let mut map = map;
        let mech = Mechanics::new(&mut map);
        (map, mech)
    }

    fn body_at(map: &Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), 0.35, 1.8).unwrap()
    }

    fn run(map: &mut Map, mech: &mut Mechanics, bodies: &mut [Body], ticks: usize) {
        for _ in 0..ticks {
            mech.tick(map, bodies, DT);
        }
    }

    #[test]
    fn doors_start_closed_and_block() {
        let (map, mech) = setup(door_rooms("(kind: Door)", ""));
        assert_eq!(map.sectors[1].ceil_z, 0.0);
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        let mut b = body_at(&map, 3.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(3.0, 0.0), 0.55);
        assert_eq!(b.sector, 0);
    }

    #[test]
    fn toggled_door_opens_fully_and_reports_changes() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door)", ""));
        mech.toggle(0);
        assert_eq!(mech.tick(&mut map, &mut [], DT), vec![1]);
        run(&mut map, &mut mech, &mut [], 80);
        assert_eq!(map.sectors[1].ceil_z, 3.0);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        assert!(
            mech.tick(&mut map, &mut [], DT).is_empty(),
            "idle movers report nothing"
        );
        let mut b = body_at(&map, 3.0, 2.0);
        clip_move(&map, &mut b, Vec2::new(3.0, 0.0), 0.55);
        assert_eq!(b.sector, 2);
    }

    #[test]
    fn door_auto_closes_after_wait() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door, auto_return: Some(1.0))", ""));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], 80 + 60 + 80);
        assert_eq!(map.sectors[1].ceil_z, 0.0);
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
    }

    #[test]
    fn toggle_mid_travel_reverses() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door)", ""));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], 20);
        mech.toggle(0);
        assert_eq!(mech.movers[0].motion, Motion::ToStart);
        run(&mut map, &mut mech, &mut [], 40);
        assert_eq!(map.sectors[1].ceil_z, 0.0);
    }

    #[test]
    fn closing_door_reopens_when_player_in_doorway() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door, auto_return: Some(0.5))", ""));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], 80);
        let mut bodies = [body_at(&map, 4.25, 2.0)];
        assert_eq!(bodies[0].sector, 1);
        let mut reopened = false;
        for _ in 0..300 {
            mech.tick(&mut map, &mut bodies, DT);
            assert!(
                map.sectors[1].ceil_z >= 1.8 - 1e-4,
                "door crushed the player: {}",
                map.sectors[1].ceil_z
            );
            reopened |= mech.movers[0].motion == Motion::ToEnd;
        }
        assert!(reopened, "door must reverse when blocked");
    }

    #[test]
    fn lift_carries_rider_up_and_down() {
        let (mut map, mut mech) = setup(lift_shaft("(kind: Lift(to: 2.0), speed: 2.0)", ""));
        let mut bodies = [body_at(&map, 5.0, 2.0)];
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, 90);
        assert_eq!((map.sectors[1].floor_z, bodies[0].pos.z), (2.0, 2.0));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, 90);
        assert_eq!((map.sectors[1].floor_z, bodies[0].pos.z), (0.0, 0.0));
    }

    #[test]
    fn descending_lift_leaves_a_rider_held_up_by_the_ledge() {
        let (mut map, mut mech) = setup(lift_shaft("(kind: Lift(to: 2.0))", ""));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], 90);
        // On the raised lift, overhanging ledge B (x ≥ 6, floor 2) by 0.05 m.
        let mut bodies = [body_at(&map, 5.7, 2.0)];
        assert_eq!(bodies[0].sector, 1);
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, 5);
        assert!(map.sectors[1].floor_z < 2.0, "lift went down");
        assert_eq!(bodies[0].pos.z, 2.0, "feet stay on the ledge");
    }

    #[test]
    fn rising_lift_catches_an_airborne_body() {
        let (mut map, mut mech) = setup(lift_shaft("(kind: Lift(to: 2.0))", ""));
        let mut bodies = [body_at(&map, 5.0, 2.0)];
        bodies[0].on_ground = false;
        bodies[0].pos.z = 0.01; // mid-jump, just above the floor
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, 10);
        assert!(
            bodies[0].pos.z >= map.sectors[1].floor_z,
            "feet {} under floor {}",
            bodies[0].pos.z,
            map.sectors[1].floor_z
        );
    }

    #[test]
    fn lift_reverses_instead_of_squeezing() {
        let (mut map, mut mech) = setup(lift_shaft("(kind: Lift(to: 2.0))", ""));
        map.sectors[0].ceil_z = 3.0; // room A has a low ceiling the rider overlaps
        let mut bodies = [body_at(&map, 4.2, 2.0)];
        assert!(touches_sector(&map, &bodies[0], 0));
        mech.toggle(0);
        for _ in 0..200 {
            mech.tick(&mut map, &mut bodies, DT);
            let b = bodies[0];
            assert!(
                b.pos.z + b.height <= 3.0 + 1e-4,
                "head in ceiling at {}",
                b.pos.z
            );
        }
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        assert_eq!(map.sectors[1].floor_z, 0.0);
    }

    #[test]
    fn fire_toggles_only_listeners() {
        let (_, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(1))",
            "switches: [(wall: (7, 0), action: Channel(1))],",
        ));
        assert_eq!(mech.fire(2), 0);
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        assert_eq!(mech.fire(1), 1);
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    }

    #[test]
    fn activate_respects_locks() {
        let (map, mut mech) = setup(door_rooms("(kind: Door, lock: Some(Red))", ""));
        assert_eq!(
            mech.activate(&map, UseTarget::Mover(0), KeySet::default()),
            UseOutcome::NeedKey(Key::Red)
        );
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        let mut keys = KeySet::default();
        keys.insert(Key::Red);
        assert_eq!(
            mech.activate(&map, UseTarget::Mover(0), keys),
            UseOutcome::Activated
        );
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    }

    #[test]
    fn switch_fires_its_channel_and_toggles_its_lamp() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(1))",
            "switches: [(wall: (7, 0), action: Channel(1))],",
        ));
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), KeySet::default()),
            UseOutcome::Activated
        );
        assert!(mech.switch_on[0]);
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    }

    #[test]
    fn keyed_exit_switch() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "switches: [(wall: (7, 0), action: Exit, key: Some(Blue))],",
        ));
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), KeySet::default()),
            UseOutcome::NeedKey(Key::Blue)
        );
        let mut keys = KeySet::default();
        keys.insert(Key::Blue);
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), keys),
            UseOutcome::Exit
        );
    }

    #[test]
    fn pickup_happens_once() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "items: [(kind: Key(Red), pos: (2.5, 2.0)), (kind: Key(Blue), pos: (3.5, 2.0))],",
        ));
        let b = body_at(&map, 2.0, 2.0);
        assert_eq!(
            mech.pickup(&map, &b, |_| true),
            vec![0],
            "only the near item"
        );
        assert!(mech.pickup(&map, &b, |_| true).is_empty(), "never twice");
        assert!(mech.taken[0] && !mech.taken[1]);
    }

    #[test]
    fn refused_item_stays_and_can_be_taken_later() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "items: [(kind: Key(Red), pos: (2.5, 2.0))],",
        ));
        let b = body_at(&map, 2.0, 2.0);
        assert!(mech.pickup(&map, &b, |_| false).is_empty());
        assert!(!mech.taken[0]);
        assert_eq!(mech.pickup(&map, &b, |_| true), vec![0]);
    }

    fn started(sector: SectorId, kind: MoverKind) -> MechEvent {
        MechEvent::MoverStarted { sector, kind }
    }

    fn stopped(sector: SectorId, kind: MoverKind) -> MechEvent {
        MechEvent::MoverStopped { sector, kind }
    }

    #[test]
    fn door_use_emits_start_stop_then_auto_close_pair() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door, auto_return: Some(1.0))", ""));
        let door = MoverKind::Door;
        mech.activate(&map, UseTarget::Mover(0), KeySet::default());
        assert_eq!(mech.drain_events(), vec![started(1, door)]);
        run(&mut map, &mut mech, &mut [], 80);
        assert_eq!(mech.drain_events(), vec![stopped(1, door)]);
        // The one-second wait is silent; the auto-close then starts the door again.
        let mut waited = 0;
        while mech.events.is_empty() {
            run(&mut map, &mut mech, &mut [], 1);
            waited += 1;
            assert!(waited < 90, "door never auto-closed");
        }
        assert!(waited >= 30, "closed after only {waited} ticks");
        assert_eq!(mech.drain_events(), vec![started(1, door)]);
        run(&mut map, &mut mech, &mut [], 80);
        assert_eq!(mech.drain_events(), vec![stopped(1, door)]);
    }

    #[test]
    fn reversal_mid_travel_emits_nothing() {
        let (mut map, mut mech) = setup(door_rooms("(kind: Door)", ""));
        mech.toggle(0);
        mech.drain_events();
        run(&mut map, &mut mech, &mut [], 20);
        mech.toggle(0);
        assert!(mech.drain_events().is_empty());
    }

    #[test]
    fn lift_switch_emits_switch_then_start() {
        let (map, mut mech) = setup(lift_shaft(
            "(kind: Lift(to: 2.0), channel: Some(1))",
            CH_SWITCH,
        ));
        mech.activate(&map, UseTarget::Switch(0), KeySet::default());
        assert_eq!(
            mech.drain_events(),
            vec![
                MechEvent::SwitchUsed(0),
                started(1, MoverKind::Lift { to: 2.0 })
            ]
        );
        assert!(mech.drain_events().is_empty(), "drain empties the queue");
    }

    #[test]
    fn missing_key_emits_need_key_only() {
        let (map, mut mech) = setup(door_rooms("(kind: Door, lock: Some(Red))", ""));
        mech.activate(&map, UseTarget::Mover(0), KeySet::default());
        assert_eq!(mech.drain_events(), vec![MechEvent::NeedKey(Key::Red)]);
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            CH_SWITCH,
        ));
        mech.activate(&map, UseTarget::Switch(0), KeySet::default());
        assert_eq!(mech.drain_events(), vec![MechEvent::NeedKey(Key::Red)]);
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "switches: [(wall: (7, 0), action: Exit, key: Some(Blue))],",
        ));
        mech.activate(&map, UseTarget::Switch(0), KeySet::default());
        assert_eq!(mech.drain_events(), vec![MechEvent::NeedKey(Key::Blue)]);
    }

    #[test]
    fn exit_switch_emits_exit() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "switches: [(wall: (7, 0), action: Exit)],",
        ));
        mech.activate(&map, UseTarget::Switch(0), KeySet::default());
        assert_eq!(
            mech.drain_events(),
            vec![MechEvent::SwitchUsed(0), MechEvent::Exit]
        );
    }

    #[test]
    fn pickup_emits_item_taken_only_when_accepted() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "items: [(kind: Key(Red), pos: (2.5, 2.0)), (kind: Key(Blue), pos: (3.5, 2.0))],",
        ));
        let b = body_at(&map, 2.0, 2.0);
        mech.pickup(&map, &b, |_| false);
        assert!(mech.drain_events().is_empty());
        mech.pickup(&map, &b, |_| true);
        assert_eq!(
            mech.drain_events(),
            vec![MechEvent::ItemTaken {
                item: 0,
                kind: ItemKind::Key(Key::Red)
            }]
        );
    }

    const CH_SWITCH: &str = "switches: [(wall: (7, 0), action: Channel(1))],";

    #[test]
    fn channel_to_locked_door_needs_key_at_switch() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            CH_SWITCH,
        ));
        let out = mech.activate(&map, UseTarget::Switch(0), KeySet::default());
        assert_eq!(out, UseOutcome::NeedKey(Key::Red));
        assert!(!mech.switch_on[0], "lamp stays off");
        assert_eq!(mech.movers[0].motion, Motion::AtStart, "nothing fired");
        let mut keys = KeySet::default();
        keys.insert(Key::Red);
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), keys),
            UseOutcome::Activated
        );
        assert!(mech.switch_on[0]);
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    }

    #[test]
    fn switch_missing_one_listener_key_fires_nothing() {
        let mut map = door_rooms("(kind: Door, channel: Some(1), lock: Some(Red))", CH_SWITCH);
        map.sectors[0].mover = Some(MoverDef {
            kind: MoverKind::Door,
            speed: 2.5,
            lock: Some(Key::Blue),
            channel: Some(1),
            auto_return: None,
            one_shot: false,
        });
        let (map, mut mech) = setup(map);
        assert_eq!(mech.movers.len(), 2);
        let mut keys = KeySet::default();
        keys.insert(Key::Red);
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), keys),
            UseOutcome::NeedKey(Key::Blue)
        );
        let mut keys = KeySet::default();
        keys.insert(Key::Blue);
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), keys),
            UseOutcome::NeedKey(Key::Red)
        );
        assert!(!mech.switch_on[0]);
        assert!(mech.movers.iter().all(|m| m.motion == Motion::AtStart));
        keys.insert(Key::Red);
        assert_eq!(
            mech.activate(&map, UseTarget::Switch(0), keys),
            UseOutcome::Activated
        );
        assert!(mech.movers.iter().all(|m| m.motion == Motion::ToEnd));
    }

    #[test]
    fn raw_fire_ignores_locks() {
        let (_, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            CH_SWITCH,
        ));
        assert_eq!(mech.fire(1), 1);
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
    }

    #[test]
    fn trigger_fires_its_channel_and_quake_once_on_entry() {
        let (map, mut mech) = setup(engine_room(""));
        assert!(!mech.enter(&map, PlayerId(0), 0));
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        assert!(!mech.enter(&map, PlayerId(0), 1));
        assert_eq!(mech.movers[0].motion, Motion::ToEnd);
        let ev = mech.drain_events();
        assert!(ev.contains(&MechEvent::QuakeStarted {
            strength: 0.8,
            duration: 2.0
        }));
        assert_eq!(ev.last(), Some(&MechEvent::TriggerFired(0)));
        mech.enter(&map, PlayerId(0), 0);
        mech.enter(&map, PlayerId(0), 1);
        assert!(mech.drain_events().is_empty(), "a once trigger stays spent");
    }

    #[test]
    fn repeating_trigger_fires_on_every_entry() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(2))",
            "triggers: [(sector: 2, action: Channel(2), once: false)],",
        ));
        mech.enter(&map, PlayerId(0), 2);
        mech.enter(&map, PlayerId(0), 0);
        mech.enter(&map, PlayerId(0), 2);
        let fired = mech
            .drain_events()
            .iter()
            .filter(|e| matches!(e, MechEvent::TriggerFired(0)))
            .count();
        assert_eq!(fired, 2);
        assert_eq!(
            mech.movers[0].motion,
            Motion::ToStart,
            "toggled twice: reversed mid-travel"
        );
    }

    #[test]
    fn second_player_entering_fires_repeatable_trigger() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(2))",
            "triggers: [(sector: 2, action: Channel(2), once: false)],",
        ));
        let fired = |mech: &mut Mechanics| {
            mech.drain_events()
                .iter()
                .filter(|e| matches!(e, MechEvent::TriggerFired(0)))
                .count()
        };
        mech.enter(&map, PlayerId(0), 2);
        assert_eq!(fired(&mut mech), 1);
        mech.enter(&map, PlayerId(1), 2);
        assert_eq!(fired(&mut mech), 1, "the second player's entry fires again");
        mech.enter(&map, PlayerId(0), 2);
        assert_eq!(fired(&mut mech), 0, "staying put does not fire");
    }

    #[test]
    fn once_trigger_fires_for_first_player_only() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(2))",
            "triggers: [(sector: 2, action: Channel(2))],",
        ));
        mech.enter(&map, PlayerId(1), 2);
        mech.enter(&map, PlayerId(0), 2);
        let fired = mech
            .drain_events()
            .iter()
            .filter(|e| matches!(e, MechEvent::TriggerFired(0)))
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn players_track_their_own_sector() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door, channel: Some(2))",
            "triggers: [(sector: 2, action: Channel(2), once: false)],",
        ));
        let fired = |mech: &mut Mechanics| {
            mech.drain_events()
                .iter()
                .filter(|e| matches!(e, MechEvent::TriggerFired(0)))
                .count()
        };
        mech.enter(&map, PlayerId(0), 2);
        assert_eq!(fired(&mut mech), 1);
        mech.enter(&map, PlayerId(1), 2);
        assert_eq!(fired(&mut mech), 1);
        mech.enter(&map, PlayerId(1), 0);
        mech.enter(&map, PlayerId(1), 2);
        assert_eq!(fired(&mut mech), 1, "player 1 re-entered once");
        mech.enter(&map, PlayerId(0), 2);
        assert_eq!(fired(&mut mech), 0, "player 0 never moved");
    }

    #[test]
    fn exit_trigger_reports_exit() {
        let (map, mut mech) = setup(door_rooms(
            "(kind: Door)",
            "triggers: [(sector: 2, action: Exit)],",
        ));
        assert!(!mech.enter(&map, PlayerId(0), 0));
        assert!(mech.enter(&map, PlayerId(0), 2));
        assert_eq!(
            mech.drain_events(),
            vec![MechEvent::Exit, MechEvent::TriggerFired(0)]
        );
    }

    #[test]
    fn trigger_on_a_lift_fires_once_while_its_rider_is_carried() {
        // The trigger sits on the lift it moves.
        let mut map = lift_shaft(
            "(kind: Lift(to: 2.0), channel: Some(5))",
            "triggers: [(sector: 1, action: Channel(5))],",
        );
        let mut mech = Mechanics::new(&mut map);
        let mut bodies = [body_at(&map, 5.0, 2.0)];
        for _ in 0..120 {
            mech.enter(&map, PlayerId(0), bodies[0].sector);
            mech.tick(&mut map, &mut bodies, DT);
        }
        assert_eq!(
            map.sectors[1].floor_z, 2.0,
            "carried to the top, never re-toggled"
        );
        assert_eq!(bodies[0].pos.z, 2.0);
        let fired = mech
            .drain_events()
            .iter()
            .filter(|e| matches!(e, MechEvent::TriggerFired(_)))
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn one_shot_mover_stays_at_its_end() {
        let (mut map, mut mech) = setup(engine_room(""));
        mech.fire(7);
        mech.toggle(0);
        assert_eq!(
            mech.movers[0].motion,
            Motion::ToEnd,
            "mid-travel toggles are ignored"
        );
        run(&mut map, &mut mech, &mut [], 150);
        assert_eq!(map.sectors[2].floor_z, -2.0);
        mech.fire(7);
        mech.toggle(0);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        run(&mut map, &mut mech, &mut [], 600);
        assert_eq!(map.sectors[2].floor_z, -2.0, "never comes back");
    }

    #[test]
    fn quake_holds_then_fades_over_the_last_30_percent() {
        let (mut map, mut mech) = setup(engine_room(""));
        assert_eq!(mech.quake_strength(), 0.0);
        mech.fire(7);
        assert_eq!(mech.quake_strength(), 0.8);
        run(&mut map, &mut mech, &mut [], 60); // 1.0 s of 2.0
        assert_eq!(mech.quake_strength(), 0.8);
        run(&mut map, &mut mech, &mut [], 48); // 1.8 s: 0.2 s left of the 0.6 s fade
        assert!(
            (mech.quake_strength() - 0.8 / 3.0).abs() < 0.02,
            "{}",
            mech.quake_strength()
        );
        run(&mut map, &mut mech, &mut [], 30);
        assert_eq!(mech.quake_strength(), 0.0);
    }

    #[test]
    fn toilet_heals_ten_up_to_100_with_a_cooldown() {
        let (mut map, mut mech) = setup(engine_room(""));
        let mut v = Vitals::new();
        v.damage(15);
        assert_eq!(mech.use_prop(0, &mut v, 0), PropOutcome::Healed(10));
        assert_eq!(v.health.hp, 95);
        assert_eq!(mech.use_prop(0, &mut v, 0), PropOutcome::Busy);
        run(&mut map, &mut mech, &mut [], 91);
        assert_eq!(mech.use_prop(0, &mut v, 0), PropOutcome::Healed(5));
        run(&mut map, &mut mech, &mut [], 91);
        assert_eq!(
            mech.use_prop(0, &mut v, 0),
            PropOutcome::Flushed,
            "flushes at full health"
        );
        let ev = mech.drain_events();
        assert_eq!(
            ev[0],
            MechEvent::PropUsed {
                prop: 0,
                kind: PropKind::Toilet,
                outcome: PropOutcome::Healed(10)
            }
        );
        assert_eq!(ev.len(), 3, "Busy is silent");
    }

    /// The engine room with an arcade cabinet (prop 3), a jukebox (prop 4) and a payphone
    /// (prop 5) added to its gag room.
    fn arcade_room() -> (Map, Mechanics) {
        let mut map = engine_room("");
        for (kind, x) in [
            (PropKind::Arcade, 14.0),
            (PropKind::Jukebox, 15.5),
            (PropKind::Payphone, 17.0),
        ] {
            map.props.push(crate::props::Prop {
                kind,
                pos: Vec2::new(x, 3.5),
                angle: 270f32.to_radians(),
                stock: 3,
                sector: 4,
            });
        }
        setup(map)
    }

    #[test]
    fn arcade_plays_a_round_then_cools_down() {
        let (mut map, mut mech) = arcade_room();
        let mut v = Vitals::new();
        assert_eq!(mech.use_prop(3, &mut v, 0), PropOutcome::Played);
        assert_eq!(mech.use_prop(3, &mut v, 0), PropOutcome::Busy);
        // Just short of 3 s it is still busy; then it takes another coin.
        run(&mut map, &mut mech, &mut [], 179);
        assert_eq!(mech.use_prop(3, &mut v, 0), PropOutcome::Busy);
        run(&mut map, &mut mech, &mut [], 2);
        assert_eq!(mech.use_prop(3, &mut v, 0), PropOutcome::Played);
        assert_eq!(v.health.hp, 100, "no gameplay effect");
        let ev = mech.drain_events();
        assert_eq!(ev.len(), 2, "Busy is silent: {ev:?}");
        assert_eq!(
            ev[0],
            MechEvent::PropUsed {
                prop: 3,
                kind: PropKind::Arcade,
                outcome: PropOutcome::Played
            }
        );
    }

    #[test]
    fn jukebox_is_busy_for_the_length_of_its_stinger() {
        let (mut map, mut mech) = arcade_room();
        let mut v = Vitals::new();
        assert_eq!(mech.use_prop(4, &mut v, 0), PropOutcome::Stinger);
        assert_eq!(mech.props[4].cooldown, JUKEBOX_SECS);
        let ticks = (JUKEBOX_SECS / DT).round() as usize;
        run(&mut map, &mut mech, &mut [], ticks - 1);
        assert_eq!(mech.use_prop(4, &mut v, 0), PropOutcome::Busy);
        run(&mut map, &mut mech, &mut [], 2);
        assert_eq!(mech.use_prop(4, &mut v, 0), PropOutcome::Stinger);
    }

    #[test]
    fn payphone_tips_once_with_the_secrets_left_then_gives_a_dial_tone() {
        let (mut map, mut mech) = arcade_room();
        let mut v = Vitals::new();
        assert_eq!(mech.use_prop(5, &mut v, 2), PropOutcome::Tip(2));
        assert_eq!(mech.use_prop(5, &mut v, 2), PropOutcome::Busy);
        run(&mut map, &mut mech, &mut [], 91);
        assert_eq!(mech.use_prop(5, &mut v, 1), PropOutcome::DialTone);
        run(&mut map, &mut mech, &mut [], 91);
        assert_eq!(mech.use_prop(5, &mut v, 0), PropOutcome::DialTone);
        // Every payphone gives its own tip once, with the count at the time of the call.
        map.props.push(map.props[5]);
        let mut mech = Mechanics::new(&mut map);
        assert_eq!(mech.use_prop(6, &mut v, 1), PropOutcome::Tip(1));
        assert_eq!(mech.use_prop(5, &mut v, 0), PropOutcome::Tip(0));
    }

    #[test]
    fn a_channel_sets_each_of_its_blasts_off_once() {
        let mut map = engine_room(
            "blasts: [(channel: 7, at: (3.0, 2.0, 1.0), radius: 2.0, damage: 30, delay: 0.25),
                      (channel: 8, at: (4.0, 2.0, 1.0), radius: 2.0, damage: 30)],",
        );
        let mut mech = Mechanics::new(&mut map);
        assert!(mech.take_blasts().is_empty());
        mech.fire(7);
        assert_eq!(mech.take_blasts(), vec![map.blasts[0]]);
        assert!(mech.take_blasts().is_empty(), "taken once");
        mech.fire(7);
        assert!(mech.take_blasts().is_empty(), "never re-armed");
        // A trigger on the channel sets it off too.
        mech.enter(&map, PlayerId(0), 0);
        mech.apply(SwitchAction::Channel(8));
        assert_eq!(mech.take_blasts(), vec![map.blasts[1]]);
    }

    #[test]
    fn vending_machine_sells_its_stock_then_is_sold_out() {
        let (_, mut mech) = setup(engine_room(""));
        let mut v = Vitals::new();
        v.damage(15);
        assert_eq!(mech.use_prop(1, &mut v, 0), PropOutcome::Dispensed(5));
        assert_eq!(mech.use_prop(1, &mut v, 0), PropOutcome::Dispensed(5));
        assert_eq!(mech.use_prop(1, &mut v, 0), PropOutcome::SoldOut);
        assert_eq!((v.health.hp, mech.props[1].stock), (95, 0));
        assert_eq!(mech.use_prop(2, &mut v, 0), PropOutcome::Racked);
    }

    const PLATFORM: &str = "(kind: Slide(by: (7.8, 0.0)), speed: 2.0)";
    /// 7.8 m at 2 m/s, plus slack.
    const RIDE: usize = 250;

    /// The x range of slider sector 2's walls.
    fn platform_x(map: &Map) -> (f32, f32) {
        map.sectors[2]
            .walls()
            .map(|w| map.walls[w].a.x)
            .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)))
    }

    #[test]
    fn slider_starts_in_place_and_travels_its_offset() {
        let (mut map, mut mech) = setup(slide_yard(PLATFORM, ""));
        let authored = map.clone();
        assert_eq!(map, authored, "new leaves a slider where it was authored");
        assert_eq!(
            (mech.movers[0].start, mech.movers[0].end, mech.movers[0].z),
            (0.0, 7.8, 0.0)
        );
        mech.toggle(0);
        let changed = mech.tick(&mut map, &mut [], DT);
        assert_eq!(
            changed,
            vec![2],
            "the track is rebuilt as the slider's neighbour"
        );
        run(&mut map, &mut mech, &mut [], RIDE);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        let (lo, hi) = platform_x(&map);
        assert!(
            (lo - 11.9).abs() < 1e-4 && (hi - 13.9).abs() < 1e-4,
            "{lo}..{hi}"
        );
        // The track's hole loop moved along: every portal is still mirrored.
        for (id, w) in map.walls.iter().enumerate() {
            if let Some(n) = w.next_wall {
                assert_eq!((map.walls[n].a, map.walls[n].b), (w.b, w.a), "wall {id}");
            }
        }
        assert_eq!(map.find_sector(Vec2::new(12.9, 2.0), None), Some(2));
        assert_eq!(map.find_sector(Vec2::new(5.1, 2.0), None), Some(1));
    }

    #[test]
    fn slider_auto_returns_exactly_home() {
        let (mut map, mut mech) = setup(slide_yard(
            "(kind: Slide(by: (7.8, 0.0)), speed: 2.0, auto_return: Some(0.5))",
            "",
        ));
        let authored = map.clone();
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], 2 * RIDE + 60);
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        assert_eq!(map, authored, "back to the authored walls, bit for bit");
    }

    #[test]
    fn one_shot_slider_stays_at_its_end() {
        let (mut map, mut mech) = setup(slide_yard(
            "(kind: Slide(by: (7.8, 0.0)), speed: 2.0, one_shot: true)",
            "",
        ));
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], RIDE);
        mech.toggle(0);
        run(&mut map, &mut mech, &mut [], RIDE);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        assert!((platform_x(&map).0 - 11.9).abs() < 1e-4);
    }

    #[test]
    fn slider_carries_a_rider_by_its_path_length() {
        let (mut map, mut mech) = setup(slide_yard(PLATFORM, ""));
        let mut bodies = [body_at(&map, 5.1, 2.0)];
        assert_eq!(bodies[0].sector, 2);
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, RIDE);
        let b = bodies[0];
        assert!(
            b.pos.truncate().distance(Vec2::new(12.9, 2.0)) < 1e-3,
            "rider at {}",
            b.pos
        );
        assert_eq!((b.sector, b.pos.z), (2, 0.0));
        // And back again.
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, RIDE);
        assert!(bodies[0].pos.truncate().distance(Vec2::new(5.1, 2.0)) < 1e-3);
    }

    #[test]
    fn overhanging_rider_is_stopped_by_the_track_wall() {
        let (mut map, mut mech) = setup(slide_yard(PLATFORM, ""));
        // On the leading edge, in front of the solid part of the pit's east wall (y < 1.5).
        let mut bodies = [body_at(&map, 6.05, 1.2)];
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, RIDE);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        let b = bodies[0];
        assert!(
            b.pos.x <= 14.0 - b.radius + 2e-3 && b.pos.x > 13.5,
            "rider at {}",
            b.pos
        );
    }

    #[test]
    fn body_in_the_path_makes_the_slider_reverse() {
        let (mut map, mut mech) = setup(slide_yard(PLATFORM, ""));
        // Standing in the pit (below the platform's floor), in its way.
        let mut bodies = [body_at(&map, 9.0, 2.0)];
        assert_eq!(bodies[0].sector, 1);
        mech.toggle(0);
        mech.drain_events();
        let mut reversed = false;
        for _ in 0..RIDE * 2 {
            mech.tick(&mut map, &mut bodies, DT);
            reversed |= mech.movers[0].motion == Motion::ToStart;
            assert!(
                cut(&map, 2, &bodies[0]) <= BLOCK_EPS,
                "slider cut into the body"
            );
        }
        assert!(reversed);
        assert_eq!(mech.movers[0].motion, Motion::AtStart);
        assert_eq!(bodies[0].pos.truncate(), Vec2::new(9.0, 2.0), "not pushed");
        let kind = mech.movers[0].def.kind;
        assert_eq!(
            mech.drain_events(),
            vec![stopped(2, kind)],
            "a reversal is silent; arriving home is not"
        );
    }

    #[test]
    fn body_in_the_pit_is_not_carried() {
        let (mut map, mut mech) = setup(slide_yard(PLATFORM, ""));
        // In the pit beside the platform's path (y < 1), never touched by it.
        let mut bodies = [body_at(&map, 5.0, 0.5)];
        mech.toggle(0);
        run(&mut map, &mut mech, &mut bodies, RIDE);
        assert!(matches!(mech.movers[0].motion, Motion::AtEnd { .. }));
        assert_eq!(bodies[0].pos.truncate(), Vec2::new(5.0, 0.5));
    }

    #[test]
    fn slider_emits_start_and_stop_like_a_lift() {
        let (mut map, mut mech) = setup(slide_yard(
            "(kind: Slide(by: (7.8, 0.0)), speed: 2.0, channel: Some(3))",
            "",
        ));
        let kind = MoverKind::Slide { by: (7.8, 0.0) };
        assert_eq!(mech.fire(3), 1);
        assert_eq!(mech.drain_events(), vec![started(2, kind)]);
        run(&mut map, &mut mech, &mut [], RIDE);
        assert_eq!(mech.drain_events(), vec![stopped(2, kind)]);
    }
}
