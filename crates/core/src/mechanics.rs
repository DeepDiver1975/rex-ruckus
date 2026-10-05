//! Moving sectors (doors and lifts), switches, the channel bus and keycard pickups.
//! Like Build's `setanimation`/`doanimations`, each mover slides one plane of its sector (a door's
//! ceiling, a lift's floor) between two heights, and carries whoever stands on it.

use crate::collide::{Body, touches_sector, z_range};
use crate::map::{
    Channel, ItemKind, Key, KeySet, Map, MoverDef, MoverKind, SectorId, SwitchAction,
};

/// Feet within this distance of a lift floor ride along with it.
const CARRY_EPS: f32 = 0.01;
/// Horizontal reach, beyond the body radius, for picking items up.
pub const PICKUP_REACH: f32 = 0.6;
/// Items more than this far above or below the feet stay put.
const PICKUP_HEIGHT: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Height of the moving plane at the start pose (door: closed; lift: authored floor).
    pub start: f32,
    pub end: f32,
    pub z: f32,
    pub motion: Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseTarget {
    Mover(usize),
    Switch(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseOutcome {
    Activated,
    NeedKey(Key),
    Exit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mechanics {
    pub movers: Vec<Mover>,
    /// Per switch: flips on each use (drives its lamp).
    pub switch_on: Vec<bool>,
    /// Per item: already picked up.
    pub taken: Vec<bool>,
    mover_of: Vec<Option<usize>>,
}

impl Mechanics {
    /// Builds the runtime state and puts every sector into its start pose (doors closed).
    pub fn new(map: &mut Map) -> Mechanics {
        let mut movers = Vec::new();
        let mut mover_of = vec![None; map.sectors.len()];
        for (s, sector) in map.sectors.iter_mut().enumerate() {
            let Some(def) = sector.mover else { continue };
            let (start, end) = match def.kind {
                MoverKind::Door => (sector.floor_z, sector.ceil_z),
                MoverKind::Lift { to } => (sector.floor_z, to),
            };
            if def.kind == MoverKind::Door {
                sector.ceil_z = start;
            }
            mover_of[s] = Some(movers.len());
            movers.push(Mover {
                sector: s,
                def,
                start,
                end,
                z: start,
                motion: Motion::AtStart,
            });
        }
        Mechanics {
            movers,
            switch_on: vec![false; map.switches.len()],
            taken: vec![false; map.items.len()],
            mover_of,
        }
    }

    pub fn mover_in(&self, s: SectorId) -> Option<usize> {
        self.mover_of.get(s).copied().flatten()
    }

    /// Sends a mover towards its other end, or reverses it mid-travel.
    pub fn toggle(&mut self, m: usize) {
        let mv = &mut self.movers[m];
        mv.motion = match mv.motion {
            Motion::AtStart | Motion::ToStart => Motion::ToEnd,
            Motion::AtEnd { .. } | Motion::ToEnd => Motion::ToStart,
        };
    }

    /// The channel bus: toggles every mover listening on `ch`. Returns how many reacted.
    pub fn fire(&mut self, ch: Channel) -> usize {
        let hits: Vec<usize> = (0..self.movers.len())
            .filter(|&m| self.movers[m].def.channel == Some(ch))
            .collect();
        for &m in &hits {
            self.toggle(m);
        }
        hits.len()
    }

    /// The first lock key (in mover order) that `keys` lacks among the movers listening on `ch`.
    fn missing_listener_key(&self, ch: Channel, keys: KeySet) -> Option<Key> {
        self.movers
            .iter()
            .filter(|m| m.def.channel == Some(ch))
            .find_map(|m| m.def.lock.filter(|&k| !keys.contains(k)))
    }

    /// Pressing use on `target` while holding `keys`.
    pub fn activate(&mut self, map: &Map, target: UseTarget, keys: KeySet) -> UseOutcome {
        match target {
            UseTarget::Mover(m) => {
                if let Some(k) = self.movers[m].def.lock
                    && !keys.contains(k)
                {
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
                    return UseOutcome::NeedKey(k);
                }
                if let SwitchAction::Channel(ch) = sw.action
                    && let Some(k) = self.missing_listener_key(ch, keys)
                {
                    return UseOutcome::NeedKey(k);
                }
                self.switch_on[i] = !self.switch_on[i];
                match sw.action {
                    SwitchAction::Channel(ch) => {
                        self.fire(ch);
                        UseOutcome::Activated
                    }
                    SwitchAction::Exit => UseOutcome::Exit,
                }
            }
        }
    }

    /// Advances every mover by `dt`. Bodies standing on a moving floor ride along. A mover that
    /// would squeeze a body against a ceiling reverses instead. Returns the sectors whose heights changed.
    pub fn tick(&mut self, map: &mut Map, bodies: &mut [Body], dt: f32) -> Vec<SectorId> {
        let mut changed = Vec::new();
        for mv in &mut self.movers {
            let goal = match &mut mv.motion {
                Motion::AtStart => continue,
                Motion::AtEnd { wait } => {
                    if let Some(limit) = mv.def.auto_return {
                        *wait += dt;
                        if *wait >= limit {
                            mv.motion = Motion::ToStart;
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
                changed.push(mv.sector);
                if z == goal {
                    mv.motion = if goal == mv.end {
                        Motion::AtEnd { wait: 0.0 }
                    } else {
                        Motion::AtStart
                    };
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
    let s = mv.sector;
    let touching: Vec<usize> = (0..bodies.len())
        .filter(|&i| touches_sector(map, &bodies[i], s))
        .collect();
    let before: Vec<f32> = touching.iter().map(|&i| squeeze(map, &bodies[i])).collect();
    let saved = bodies.to_vec();
    let (old_floor, old_ceil) = (map.sectors[s].floor_z, map.sectors[s].ceil_z);
    match mv.def.kind {
        MoverKind::Door => map.sectors[s].ceil_z = z,
        MoverKind::Lift { .. } => {
            map.sectors[s].floor_z = z;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::clip_move;
    use crate::fixtures::{door_rooms, lift_shaft};
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
}
