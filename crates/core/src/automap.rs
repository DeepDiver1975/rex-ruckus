//! The automap: which walls the player has seen, and the lines to draw for them. Pure; the game
//! draws the lines with gizmos (`rr-game` `automap.rs`).

use crate::hazard::HazardKind;
use crate::map::{Key, Map, MoverKind, OPEN_GAP, SectorId};
use crate::mechanics::Mechanics;
use crate::trace::can_see;
use glam::{Vec2, Vec3};
use std::collections::VecDeque;

/// Portals further than this from the eye are not revealed (metres).
pub const REVEAL_RANGE: f32 = 40.0;
/// Floor differences above this draw a step line.
const STEP_LINE: f32 = 0.3;

#[derive(Debug, Clone, PartialEq)]
pub struct Automap {
    /// Per wall: seen at least once.
    pub seen: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineKind {
    Solid,
    Step,
    /// A door sector's portal, with its lock.
    Door(Option<Key>),
    /// The rim of a hazard floor.
    Hazard(HazardKind),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapLine {
    pub a: Vec2,
    pub b: Vec2,
    pub kind: LineKind,
}

impl Automap {
    pub fn new(map: &Map) -> Automap {
        Automap {
            seen: vec![false; map.walls.len()],
        }
    }

    fn mark(&mut self, map: &Map, s: SectorId) {
        for w in map.sectors[s].walls() {
            self.seen[w] = true;
        }
    }

    /// Marks the walls of `sector`, then of every sector reached through portals that are open
    /// (more than `OPEN_GAP` tall, no intact glass) and whose midpoint or an end is in clear
    /// sight of `eye` within `REVEAL_RANGE`. Closed doors therefore hide what is behind them.
    pub fn reveal(&mut self, map: &Map, eye: Vec3, sector: SectorId) {
        let mut visited = vec![false; map.sectors.len()];
        visited[sector] = true;
        self.mark(map, sector);
        let mut queue = VecDeque::from([sector]);
        while let Some(s) = queue.pop_front() {
            for w in map.sectors[s].walls() {
                let wall = &map.walls[w];
                let Some(n) = wall.passage() else { continue };
                if visited[n] {
                    continue;
                }
                let (here, there) = (&map.sectors[s], &map.sectors[n]);
                let (lo, hi) = (
                    here.floor_z.max(there.floor_z),
                    here.ceil_z.min(there.ceil_z),
                );
                if hi - lo <= OPEN_GAP {
                    continue;
                }
                let mid = (wall.a + wall.b) * 0.5;
                let z = (lo + hi) * 0.5;
                let visible = [mid, wall.a.lerp(mid, 0.05), wall.b.lerp(mid, 0.05)]
                    .into_iter()
                    .map(|p| p.extend(z))
                    .any(|p| p.distance(eye) <= REVEAL_RANGE && can_see(map, eye, sector, p));
                if visible {
                    visited[n] = true;
                    self.mark(map, n);
                    queue.push_back(n);
                }
            }
        }
    }
}

/// One line per seen wall (one per portal pair): solid walls and glass, door portals (with
/// their lock), the rims of hazard floors, steps over `STEP_LINE`, and portals into a closed
/// sector (a sealed crack). Flat open portals draw nothing. Secret sectors are not flagged.
pub fn automap_lines(map: &Map, mech: &Mechanics, automap: &Automap) -> Vec<MapLine> {
    let door = |s: SectorId| {
        mech.mover_in(s)
            .map(|m| mech.movers[m].def)
            .filter(|d| d.kind == MoverKind::Door)
    };
    let mut out = Vec::new();
    for (id, w) in map.walls.iter().enumerate() {
        if w.next_wall.is_some_and(|b| b < id) {
            continue;
        }
        if !(automap.seen[id] || w.next_wall.is_some_and(|b| automap.seen[b])) {
            continue;
        }
        let kind = match w.next_sector {
            None => LineKind::Solid,
            Some(n) => {
                let (a, b) = (&map.sectors[w.sector], &map.sectors[n]);
                if let Some(d) = door(w.sector).or(door(n)) {
                    LineKind::Door(d.lock)
                } else if let (Some(h), None) | (None, Some(h)) = (a.hazard, b.hazard) {
                    LineKind::Hazard(h.kind)
                } else if w.glass
                    || a.ceil_z - a.floor_z <= OPEN_GAP
                    || b.ceil_z - b.floor_z <= OPEN_GAP
                {
                    LineKind::Solid
                } else if (a.floor_z - b.floor_z).abs() > STEP_LINE {
                    LineKind::Step
                } else {
                    continue;
                }
            }
        };
        out.push(MapLine {
            a: w.a,
            b: w.b,
            kind,
        });
    }
    out
}

/// Screen offset (pixels, y up) of map point `p` on an automap centred on `player` and turned
/// so `heading` (core radians) points up.
pub fn to_screen(p: Vec2, player: Vec2, heading: f32, px_per_m: f32) -> Vec2 {
    let d = p - player;
    let fwd = Vec2::from_angle(heading);
    let right = Vec2::new(fwd.y, -fwd.x);
    Vec2::new(d.dot(right), d.dot(fwd)) * px_per_m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{door_rooms, engine_room, lift_shaft};

    fn sector_seen(map: &Map, am: &Automap, s: SectorId) -> bool {
        map.sectors[s].walls().all(|w| am.seen[w])
    }

    #[test]
    fn closed_door_hides_the_room_behind_it() {
        let mut map = door_rooms("(kind: Door)", "");
        let mut mech = Mechanics::new(&mut map);
        let mut am = Automap::new(&map);
        let eye = Vec3::new(2.0, 2.0, 1.6);
        am.reveal(&map, eye, 0);
        assert!(sector_seen(&map, &am, 0));
        assert!(!map.sectors[2].walls().any(|w| am.seen[w]));
        mech.toggle(0);
        for _ in 0..90 {
            mech.tick(&mut map, &mut [], 1.0 / 60.0);
        }
        am.reveal(&map, eye, 0);
        assert!(sector_seen(&map, &am, 2), "the open door shows room B");
    }

    #[test]
    fn nothing_seen_draws_nothing() {
        let mut map = engine_room("");
        let mech = Mechanics::new(&mut map);
        assert!(automap_lines(&map, &mech, &Automap::new(&map)).is_empty());
    }

    #[test]
    fn lines_mark_walls_steps_doors_and_hazards() {
        let mut map = engine_room("");
        let mech = Mechanics::new(&mut map);
        let mut am = Automap::new(&map);
        am.seen.fill(true);
        let lines = automap_lines(&map, &mech, &am);
        let at_x = |x: f32| {
            lines
                .iter()
                .find(|l| l.a.x == x && l.b.x == x)
                .map(|l| l.kind)
        };
        assert_eq!(at_x(6.0), None, "flat open portal: no line");
        assert_eq!(at_x(9.0), Some(LineKind::Hazard(HazardKind::Slime)));
        assert_eq!(at_x(13.0), Some(LineKind::Hazard(HazardKind::Slime)));
        assert_eq!(
            lines.iter().filter(|l| l.kind == LineKind::Solid).count(),
            12
        );

        let mut doors = door_rooms(
            "(kind: Door, lock: Some(Red))",
            "items: [(kind: Key(Red), pos: (1.0, 1.0))],",
        );
        let mech = Mechanics::new(&mut doors);
        let mut am = Automap::new(&doors);
        am.seen.fill(true);
        assert!(
            automap_lines(&doors, &mech, &am)
                .iter()
                .any(|l| l.kind == LineKind::Door(Some(Key::Red)))
        );

        let mut lift = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mech = Mechanics::new(&mut lift);
        let mut am = Automap::new(&lift);
        am.seen.fill(true);
        let steps: Vec<_> = automap_lines(&lift, &mech, &am)
            .into_iter()
            .filter(|l| l.kind == LineKind::Step)
            .collect();
        assert_eq!(steps.len(), 1, "lift (floor 0) to ledge (floor 2)");
        assert_eq!(steps[0].a.x, 6.0);
    }

    #[test]
    fn view_turns_with_the_player_forward_up() {
        let px = 10.0;
        assert_eq!(
            to_screen(Vec2::new(1.0, 0.0), Vec2::ZERO, 0.0, px),
            Vec2::new(0.0, 10.0)
        );
        let left = to_screen(Vec2::new(0.0, 1.0), Vec2::ZERO, 0.0, px);
        assert!(
            (left - Vec2::new(-10.0, 0.0)).length() < 1e-5,
            "north is left when facing east"
        );
        let ahead = to_screen(
            Vec2::new(5.0, 7.0),
            Vec2::new(5.0, 5.0),
            std::f32::consts::FRAC_PI_2,
            px,
        );
        assert!((ahead - Vec2::new(0.0, 20.0)).length() < 1e-4);
    }
}
