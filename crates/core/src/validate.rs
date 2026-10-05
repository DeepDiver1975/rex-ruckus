//! Level checks for `rr-tools validate` and CI: geometry sanity, trigger wiring, and whether the
//! exit (and every keycard) can be reached from the start. Run it on an authored map (straight
//! from `Map::from_raw`, doors open), never on a live one.

use crate::defs::Defs;
use crate::geom::closest_point_on_segment;
use crate::map::{
    Channel, ItemKind, Key, KeySet, Map, MoverKind, SectorId, Switch, SwitchAction, Wall, WallId,
};
use crate::movement::{Pose, Tuning, can_cross};
use glam::Vec2;
use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{level}: {}", self.message)
    }
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

#[derive(Default)]
struct Report(Vec<Issue>);

impl Report {
    fn error(&mut self, message: String) {
        self.0.push(Issue {
            severity: Severity::Error,
            message,
        });
    }
    fn warn(&mut self, message: String) {
        self.0.push(Issue {
            severity: Severity::Warning,
            message,
        });
    }
}

pub fn validate(map: &Map) -> Vec<Issue> {
    let mut r = Report::default();
    check_winding(map, &mut r);
    check_portals(map, &mut r);
    check_overlaps(map, &mut r);
    check_wiring(map, &mut r);
    let reached = check_reachability(map, &mut r);
    check_actors(map, reached.as_deref(), &mut r);
    r.0
}

fn signed_area(map: &Map, lp: &[WallId]) -> f32 {
    lp.iter()
        .map(|&w| map.walls[w].a.perp_dot(map.walls[w].b))
        .sum::<f32>()
        * 0.5
}

fn check_winding(map: &Map, r: &mut Report) {
    for (s, sec) in map.sectors.iter().enumerate() {
        for (i, lp) in sec.loops.iter().enumerate() {
            let area = signed_area(map, lp);
            if i == 0 && area <= 0.0 {
                r.error(format!("sector {s}: outer loop must be counter-clockwise"));
            } else if i > 0 && area >= 0.0 {
                r.error(format!("sector {s}: hole loop {i} must be clockwise"));
            }
        }
    }
}

/// Length over which `q` lies on `p`'s line and overlaps it (0 if not collinear).
fn collinear_overlap(p: &Wall, q: &Wall) -> f32 {
    let d = p.b - p.a;
    let len = d.length();
    if len < 1e-6 {
        return 0.0;
    }
    let u = d / len;
    let off = |x: Vec2| u.perp_dot(x - p.a).abs();
    if off(q.a) > 1e-4 || off(q.b) > 1e-4 {
        return 0.0;
    }
    let (t0, t1) = (u.dot(q.a - p.a), u.dot(q.b - p.a));
    (t0.max(t1).min(len) - t0.min(t1).max(0.0)).max(0.0)
}

fn check_portals(map: &Map, r: &mut Report) {
    for (id, w) in map.walls.iter().enumerate() {
        if let Some(n) = w.next_wall {
            let m = &map.walls[n];
            if m.next_wall != Some(id) || m.a != w.b || m.b != w.a {
                r.error(format!("wall {id}: portal is not mirrored by wall {n}"));
            }
        }
    }
    let solid: Vec<usize> = (0..map.walls.len())
        .filter(|&w| map.walls[w].next_sector.is_none())
        .collect();
    for (k, &i) in solid.iter().enumerate() {
        for &j in &solid[k + 1..] {
            let (wi, wj) = (&map.walls[i], &map.walls[j]);
            if wi.sector != wj.sector && collinear_overlap(wi, wj) > 1e-3 {
                r.error(format!(
                    "sectors {} and {} share an edge that is split at different vertices (walls {i}, {j}); use the same vertices on both sides",
                    wi.sector, wj.sector
                ));
            }
        }
    }
}

fn orient(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

/// Proper crossing: the segments intersect at a single interior point of both.
fn segments_cross(p: &Wall, q: &Wall) -> bool {
    const E: f32 = 1e-6;
    let opposite = |x: f32, y: f32| (x > E && y < -E) || (x < -E && y > E);
    opposite(orient(q.a, q.b, p.a), orient(q.a, q.b, p.b))
        && opposite(orient(p.a, p.b, q.a), orient(p.a, p.b, q.b))
}

fn check_overlaps(map: &Map, r: &mut Report) {
    let mut pairs: BTreeSet<(SectorId, SectorId)> = BTreeSet::new();
    for s in 0..map.sectors.len() {
        for wid in map.sectors[s].walls() {
            let w = &map.walls[wid];
            let inside = (w.a + w.b) * 0.5 + w.inward_normal() * 0.01;
            for t in (0..map.sectors.len()).filter(|&t| t != s) {
                if map.sector_contains(t, inside) {
                    pairs.insert((s.min(t), s.max(t)));
                }
            }
        }
    }
    for (i, wi) in map.walls.iter().enumerate() {
        for wj in &map.walls[i + 1..] {
            if wi.sector != wj.sector && segments_cross(wi, wj) {
                pairs.insert((wi.sector.min(wj.sector), wi.sector.max(wj.sector)));
            }
        }
    }
    for (s, t) in pairs {
        r.error(format!("sectors {s} and {t} overlap"));
    }
}

fn check_wiring(map: &Map, r: &mut Report) {
    let t = Tuning::default();
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    match map.find_sector(start, None) {
        None => r.error(format!("player_start {start} is outside every sector")),
        Some(s) if map.sectors[s].ceil_z - map.sectors[s].floor_z < t.stand_height => r.error(
            format!("player_start {start}: sector {s} is too low to stand in"),
        ),
        Some(_) => {}
    }
    let has_key = |k: Key| map.items.iter().any(|i| i.kind == ItemKind::Key(k));
    for (s, sec) in map.sectors.iter().enumerate() {
        let Some(def) = sec.mover else { continue };
        if def.kind == MoverKind::Crack && !sec.walls().any(|w| map.walls[w].next_sector.is_some())
        {
            r.error(format!("sector {s}: crack wall has no portal to open"));
        }
        if let Some(c) = def.channel
            && !map
                .switches
                .iter()
                .any(|sw| sw.action == SwitchAction::Channel(c))
        {
            r.error(format!(
                "sector {s}: mover listens on channel {c}, but no switch fires it"
            ));
        }
        if let Some(k) = def.lock
            && !has_key(k)
        {
            r.error(format!(
                "sector {s}: locked with the {} keycard, but the level has none",
                k.name()
            ));
        }
    }
    for (i, sw) in map.switches.iter().enumerate() {
        if map.walls[sw.wall].next_sector.is_some() {
            r.error(format!("switch {i} is mounted on a portal wall"));
        }
        if let SwitchAction::Channel(c) = sw.action
            && !map
                .sectors
                .iter()
                .any(|s| s.mover.is_some_and(|m| m.channel == Some(c)))
        {
            r.warn(format!("switch {i}: channel {c} has no listeners"));
        }
        if let Some(k) = sw.key
            && !has_key(k)
        {
            r.error(format!(
                "switch {i}: needs the {} keycard, but the level has none",
                k.name()
            ));
        }
    }
    for (i, item) in map.items.iter().enumerate() {
        if map.find_sector(item.pos, None).is_none() {
            r.error(format!("item {i} at {} is outside every sector", item.pos));
        }
    }
    if !map
        .switches
        .iter()
        .any(|sw| sw.action == SwitchAction::Exit)
    {
        r.warn("level has no exit switch".into());
    }
}

/// Floor/ceiling pairs a sector can offer a player, given the keys held and channels fired.
fn poses(map: &Map, s: SectorId, keys: KeySet, fired: &[Channel]) -> Vec<Pose> {
    let sec = &map.sectors[s];
    let Some(def) = sec.mover else {
        return vec![(sec.floor_z, sec.ceil_z)];
    };
    let operable = match def.channel {
        Some(c) => fired.contains(&c),
        None => def.lock.is_none_or(|k| keys.contains(k)),
    };
    match (def.kind, operable) {
        (MoverKind::Door, true) => vec![(sec.floor_z, sec.ceil_z)],
        (MoverKind::Door, false) => vec![],
        (MoverKind::Lift { to }, true) => vec![(sec.floor_z, sec.ceil_z), (to, sec.ceil_z)],
        (MoverKind::Lift { .. }, false) => vec![(sec.floor_z, sec.ceil_z)],
        // Only a blast opens it, and no level may depend on one.
        (MoverKind::Crack, _) => vec![],
    }
}

fn flood(map: &Map, start: SectorId, keys: KeySet, fired: &[Channel], t: &Tuning) -> Vec<bool> {
    let mut reached = vec![false; map.sectors.len()];
    reached[start] = true;
    let mut queue = vec![start];
    while let Some(s) = queue.pop() {
        let from = poses(map, s, keys, fired);
        for n in map.neighbours(s) {
            if reached[n] {
                continue;
            }
            let to = poses(map, n, keys, fired);
            if from.iter().any(|&a| to.iter().any(|&b| can_cross(a, b, t))) {
                reached[n] = true;
                queue.push(n);
            }
        }
    }
    reached
}

/// Reports unreachable items and exits; returns the final reachable-sector set (None when the
/// start is outside every sector).
fn check_reachability(map: &Map, r: &mut Report) -> Option<Vec<bool>> {
    let t = Tuning::default();
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    let Some(start) = map.find_sector(start, None) else {
        return None; // reported by check_wiring
    };
    // A channel switch also needs the lock key of every mover listening on its channel.
    let usable = |sw: &Switch, keys: KeySet| {
        sw.key.is_none_or(|k| keys.contains(k))
            && match sw.action {
                SwitchAction::Channel(c) => map
                    .sectors
                    .iter()
                    .filter_map(|s| s.mover)
                    .filter(|m| m.channel == Some(c))
                    .all(|m| m.lock.is_none_or(|k| keys.contains(k))),
                SwitchAction::Exit => true,
            }
    };
    let mut keys = KeySet::default();
    let mut fired: Vec<Channel> = Vec::new();
    let reached = loop {
        let reached = flood(map, start, keys, &fired, &t);
        let mut changed = false;
        for item in &map.items {
            let ItemKind::Key(k) = item.kind else {
                continue;
            };
            if map.find_sector(item.pos, None).is_some_and(|s| reached[s]) && !keys.contains(k) {
                keys.insert(k);
                changed = true;
            }
        }
        for sw in &map.switches {
            if let SwitchAction::Channel(c) = sw.action
                && reached[map.walls[sw.wall].sector]
                && usable(sw, keys)
                && !fired.contains(&c)
            {
                fired.push(c);
                changed = true;
            }
        }
        if !changed {
            break reached;
        }
    };
    for (i, item) in map.items.iter().enumerate() {
        if !map.find_sector(item.pos, None).is_some_and(|s| reached[s]) {
            r.warn(format!(
                "item {i} ({:?}) cannot be reached from the start",
                item.kind
            ));
        }
    }
    let exits: Vec<&Switch> = map
        .switches
        .iter()
        .filter(|sw| sw.action == SwitchAction::Exit)
        .collect();
    if !exits.is_empty()
        && !exits
            .iter()
            .any(|sw| reached[map.walls[sw.wall].sector] && usable(sw, keys))
    {
        r.error("no exit switch can be reached and used from the start".into());
    }
    Some(reached)
}

/// Awake actors closer than this (straight-line XY, metres) to the player start get a warning.
const NEAR_START: f32 = 3.0;

/// Actor spawns. Sizes come from `Defs::builtin()`. "Blocking wall" means any wall of the actor's
/// own sector with no `passage` (a solid wall or a glass pane); open portal walls never block,
/// matching how the start's room-to-stand check treats geometry (it only looks at sector
/// height).
fn check_actors(map: &Map, reached: Option<&[bool]>, r: &mut Report) {
    let defs = Defs::builtin();
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    for (i, a) in map.actors.iter().enumerate() {
        let at = format!("({:?}, {:?})", a.pos.x, a.pos.y);
        let who = format!("actor {i} ({:?}) at {at}", a.kind);
        let Some(s) = map.find_sector(a.pos, None) else {
            r.error(format!("{who} is outside every sector"));
            continue;
        };
        let def = defs.enemy(a.kind);
        let sec = &map.sectors[s];
        if sec.ceil_z - sec.floor_z < def.height {
            r.error(format!(
                "{who}: sector {s} is too low for its height ({} m)",
                def.height
            ));
        }
        if sec.mover.is_some_and(|m| matches!(m.kind, MoverKind::Door)) {
            r.error(format!("{who} is in a door sector ({s})"));
        }
        let near_wall = sec.walls().any(|w| {
            let wall = &map.walls[w];
            wall.passage().is_none()
                && closest_point_on_segment(a.pos, wall.a, wall.b).distance(a.pos) < def.radius
        });
        if near_wall {
            r.error(format!(
                "{who} is within {} m of a blocking wall",
                def.radius
            ));
        }
        if reached.is_some_and(|re| !re[s]) {
            r.warn(format!("{who} cannot be reached from the start"));
        }
        if !a.asleep && a.pos.distance(start) < NEAR_START {
            r.warn(format!(
                "awake actor {i} ({:?}) at {at} is within {NEAR_START} m of the player start",
                a.kind
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{combat_room, door_rooms, lift_shaft, pillar_room, two_rooms};
    use crate::map::{ActorKind, ActorSpawn};

    fn errors(map: &Map) -> Vec<String> {
        validate(map)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.message)
            .collect()
    }
    fn warnings(map: &Map) -> Vec<String> {
        validate(map)
            .into_iter()
            .filter(|i| i.severity == Severity::Warning)
            .map(|i| i.message)
            .collect()
    }
    fn has(list: &[String], needle: &str) -> bool {
        list.iter().any(|m| m.contains(needle))
    }
    fn raw(vertices: &str, sectors: &str, extra: &str) -> Map {
        Map::from_ron(&format!(
            r#"(name: "t", materials: ["m"], vertices: [{vertices}], sectors: [{sectors}],
                player_start: (pos: (1.0, 1.0), angle_deg: 0.0), {extra})"#
        ))
        .unwrap()
    }
    const SQ: &str = "floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0";

    #[test]
    fn fixtures_have_no_errors() {
        for map in [
            two_rooms(0.4, 3.0),
            pillar_room(),
            door_rooms("(kind: Door)", ""),
            combat_room(),
        ] {
            assert_eq!(errors(&map), Vec::<String>::new(), "{}", map.name);
            assert!(has(&warnings(&map), "no exit"), "{}", map.name);
        }
    }

    #[test]
    fn issue_display_names_severity() {
        let i = Issue {
            severity: Severity::Error,
            message: "boom".into(),
        };
        assert_eq!(i.to_string(), "error: boom");
        assert!(has_errors(&[i]));
    }

    #[test]
    fn clockwise_outer_loop_is_an_error() {
        let map = raw(
            "(0.0,0.0),(4.0,0.0),(4.0,4.0)",
            &format!("(loops: [[0,2,1]], {SQ})"),
            "",
        );
        assert!(has(&errors(&map), "counter-clockwise"));
    }

    #[test]
    fn edge_split_differently_on_each_side_is_an_error() {
        let map = raw(
            "(0.0,0.0),(4.0,0.0),(4.0,4.0),(0.0,4.0),(8.0,0.0),(8.0,4.0),(4.0,2.0)",
            &format!("(loops: [[0,1,2,3]], {SQ}), (loops: [[1,4,5,2,6]], {SQ})"),
            "",
        );
        assert!(has(&errors(&map), "split at different vertices"));
    }

    #[test]
    fn overlapping_sectors_are_an_error() {
        let map = raw(
            "(0.0,0.0),(4.0,0.0),(4.0,4.0),(0.0,4.0),(2.0,2.0),(6.0,2.0),(6.0,6.0),(2.0,6.0)",
            &format!("(loops: [[0,1,2,3]], {SQ}), (loops: [[4,5,6,7]], {SQ})"),
            "",
        );
        assert!(has(&errors(&map), "overlap"));
    }

    #[test]
    fn start_outside_or_cramped_is_an_error() {
        let mut map = two_rooms(0.0, 3.0);
        map.player_start.pos = (50.0, 50.0);
        assert!(has(&errors(&map), "player_start"));
        let mut map = two_rooms(0.0, 3.0);
        map.sectors[0].ceil_z = 1.5;
        assert!(has(&errors(&map), "player_start"));
    }

    #[test]
    fn wiring_mistakes_are_reported() {
        assert!(has(
            &errors(&door_rooms("(kind: Door, channel: Some(3))", "")),
            "channel 3"
        ));
        assert!(has(
            &warnings(&door_rooms(
                "(kind: Door)",
                "switches: [(wall: (7, 0), action: Channel(5))],"
            )),
            "channel 5"
        ));
        assert!(has(
            &errors(&door_rooms("(kind: Door, lock: Some(Yellow))", "")),
            "yellow"
        ));
        assert!(has(
            &errors(&door_rooms(
                "(kind: Door)",
                "switches: [(wall: (1, 6), action: Exit)],"
            )),
            "portal"
        ));
        assert!(has(
            &errors(&door_rooms(
                "(kind: Door)",
                "items: [(kind: Key(Red), pos: (50.0, 50.0))],"
            )),
            "outside"
        ));
        assert!(has(
            &errors(&door_rooms(
                "(kind: Door)",
                "switches: [(wall: (7, 0), action: Exit, key: Some(Blue))],"
            )),
            "blue"
        ));
    }

    #[test]
    fn locked_exit_with_reachable_key_is_clean() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Red))",
            "switches: [(wall: (3, 4), action: Exit)], items: [(kind: Key(Red), pos: (2.0, 3.0))],",
        );
        assert_eq!(validate(&map), vec![]);
    }

    #[test]
    fn key_behind_its_own_door_is_unreachable() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Red))",
            "switches: [(wall: (3, 4), action: Exit)], items: [(kind: Key(Red), pos: (6.5, 2.0))],",
        );
        assert!(has(&errors(&map), "exit"));
        assert!(has(&warnings(&map), "item 0"));
    }

    #[test]
    fn exit_behind_crack_rejected() {
        let behind = door_rooms("(kind: Crack)", "switches: [(wall: (3, 4), action: Exit)],");
        assert!(has(&errors(&behind), "exit"), "{:?}", errors(&behind));
        // A key behind the crack cannot unlock anything either.
        let keyed = door_rooms(
            "(kind: Crack)",
            "switches: [(wall: (3, 4), action: Exit, key: Some(Red))], items: [(kind: Key(Red), pos: (6.5, 2.0))],",
        );
        assert!(has(&errors(&keyed), "exit"));
        // The exit on the near side is fine, however the crack sits.
        let near = door_rooms("(kind: Crack)", "switches: [(wall: (7, 0), action: Exit)],");
        assert_eq!(validate(&near), vec![]);
    }

    #[test]
    fn crack_needs_a_portal() {
        let map = raw(
            "(0.0,0.0),(4.0,0.0),(4.0,4.0),(0.0,4.0)",
            &format!("(loops: [[0,1,2,3]], {SQ}, mover: Some((kind: Crack)))"),
            "",
        );
        assert!(has(&errors(&map), "crack"), "{:?}", errors(&map));
    }

    #[test]
    fn remote_door_opens_when_its_switch_is_reachable() {
        let map = door_rooms(
            "(kind: Door, channel: Some(1))",
            "switches: [(wall: (7, 0), action: Channel(1)), (wall: (3, 4), action: Exit)],",
        );
        assert_eq!(validate(&map), vec![]);
    }

    #[test]
    fn lift_reaches_ledge_only_when_operable() {
        let ok = lift_shaft(
            "(kind: Lift(to: 2.0))",
            "switches: [(wall: (3, 4), action: Exit)],",
        );
        assert_eq!(validate(&ok), vec![]);
        let stuck = lift_shaft(
            "(kind: Lift(to: 2.0), channel: Some(9))",
            "switches: [(wall: (3, 4), action: Exit)],",
        );
        assert!(has(&errors(&stuck), "exit"));
    }

    #[test]
    fn remote_locked_door_needs_its_key() {
        // The only way to open the door is a remote switch, and that switch needs the door's own
        // red card, which lies behind the door.
        let map = door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            "switches: [(wall: (7, 0), action: Channel(1)), (wall: (3, 4), action: Exit)], items: [(kind: Key(Red), pos: (6.5, 2.0))],",
        );
        assert!(has(&errors(&map), "exit"));
        assert!(has(&warnings(&map), "item 0"));
        // With the key on the near side the same wiring is fine.
        let ok = door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            "switches: [(wall: (7, 0), action: Channel(1)), (wall: (3, 4), action: Exit)], items: [(kind: Key(Red), pos: (2.0, 3.0))],",
        );
        assert_eq!(validate(&ok), vec![]);
    }

    fn actor(x: f32, y: f32, asleep: bool) -> String {
        format!("(kind: Grunt, pos: ({x:?}, {y:?}), angle_deg: 0.0, asleep: {asleep})")
    }

    #[test]
    fn actor_outside_or_in_wall_is_an_error() {
        let out = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(50.0, 50.0, true)),
        );
        assert!(has(
            &errors(&out),
            "actor 0 (Grunt) at (50.0, 50.0) is outside every sector"
        ));
        // 0.2 m from the west wall of room A: inside the 0.35 m radius.
        let near = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(0.2, 2.0, true)),
        );
        assert!(
            has(&errors(&near), "actor 0 (Grunt) at (0.2, 2.0)"),
            "{:?}",
            errors(&near)
        );
        assert!(has(&errors(&near), "wall"));
        // Clear of every wall: fine.
        let ok = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(2.0, 2.0, true)),
        );
        assert_eq!(errors(&ok), Vec::<String>::new());
        // Too low for a 1.75 m grunt.
        let mut low = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(2.0, 2.0, true)),
        );
        low.sectors[0].ceil_z = 1.5;
        assert!(has(&errors(&low), "too low"));
    }

    #[test]
    fn actor_in_door_sector_is_an_error() {
        let map = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(4.25, 2.0, true)),
        );
        assert!(
            has(
                &errors(&map),
                "actor 0 (Grunt) at (4.25, 2.0) is in a door sector"
            ),
            "{:?}",
            errors(&map)
        );
    }

    #[test]
    fn unreachable_actor_warns() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Red))",
            &format!("actors: [{}],", actor(6.5, 2.0, true)),
        );
        assert!(has(
            &warnings(&map),
            "actor 0 (Grunt) at (6.5, 2.0) cannot be reached from the start"
        ));
        let open = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(6.5, 2.0, true)),
        );
        assert!(!has(&warnings(&open), "actor 0"));
    }

    #[test]
    fn awake_actor_near_start_warns() {
        let awake = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(2.0, 2.0, false)),
        );
        assert!(has(&warnings(&awake), "awake actor 0"));
        let asleep = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(2.0, 2.0, true)),
        );
        assert!(!has(&warnings(&asleep), "actor 0"));
        let far = door_rooms(
            "(kind: Door)",
            &format!("actors: [{}],", actor(6.5, 2.0, false)),
        );
        assert!(!has(&warnings(&far), "awake actor 0"));
    }

    #[test]
    fn actor_near_pillar_wall_is_an_error() {
        let mut map = pillar_room();
        let spawn = |x, y| ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(x, y),
            angle: 0.0,
            asleep: true,
        };
        // Pillar occupies x 4..6, y 4..6: 0.2 m from its west face is inside the 0.35 m radius.
        map.actors = vec![spawn(3.8, 5.0), spawn(3.0, 5.0)];
        let errs = errors(&map);
        assert!(
            has(&errs, "actor 0 (Grunt) at (3.8, 5.0) is within"),
            "{errs:?}"
        );
        assert!(!has(&errs, "actor 1"), "{errs:?}");
    }
}
