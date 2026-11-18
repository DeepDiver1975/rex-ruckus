//! Level checks for `rr-tools validate` and CI: geometry sanity, trigger wiring, and whether the
//! exit (and every keycard) can be reached from the start. Run it on an authored map (straight
//! from `Map::from_raw`, doors open), never on a live one.
//!
//! Reachability never counts on a barrel or a rocket to open a crack wall; a crack that a
//! scripted blast (`Map::blasts`) on a channel the player can fire reaches is passable.

use crate::actors::steer::seg_seg_dist;
use crate::defs::{Defs, SplashDef};
use crate::difficulty::Difficulty;
use crate::explosion::{Blast, sectors_in_reach};
use crate::geom::closest_point_on_segment;
use crate::map::{
    ActorKind, Channel, ItemKind, Key, KeySet, Map, MoverKind, SectorId, Switch, SwitchAction,
    Wall, WallId,
};
use crate::mechanics::{Mechanics, channel_required_keys};
use crate::movement::{Pose, Tuning, can_cross};
use crate::projectile::Shooter;
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
    check_sliders(map, &mut r);
    let reached = check_reachability(map, &mut r);
    check_actors(map, reached.as_deref(), &mut r);
    check_burrow_points(map, reached.as_deref(), &mut r);
    check_props(map, &mut r);
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

/// Who can fire channel `c`: switches, triggers and `on_death` actions.
fn channel_sourced(map: &Map, c: Channel) -> bool {
    let fires = |a: SwitchAction| a == SwitchAction::Channel(c);
    map.switches.iter().any(|s| fires(s.action))
        || map.triggers.iter().any(|t| fires(t.action))
        || map.actors.iter().any(|a| a.on_death.is_some_and(fires))
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
    for (i, item) in map.items.iter().enumerate() {
        if matches!(item.kind, ItemKind::Key(_)) && item.skill != Difficulty::Easy {
            r.error(format!(
                "item {i}: keycard must appear on every skill (skill: Easy)"
            ));
        }
    }
    for (s, sec) in map.sectors.iter().enumerate() {
        let Some(def) = sec.mover else { continue };
        if def.kind == MoverKind::Crack && !sec.walls().any(|w| map.walls[w].next_sector.is_some())
        {
            r.error(format!("sector {s}: crack wall has no portal to open"));
        }
        if let Some(c) = def.channel
            && !channel_sourced(map, c)
        {
            r.error(format!(
                "sector {s}: mover listens on channel {c}, but nothing fires it"
            ));
        }
        if def.one_shot
            && let Some(c) = def.channel
            && map
                .switches
                .iter()
                .any(|sw| sw.action == SwitchAction::Channel(c))
        {
            r.warn(format!(
                "sector {s}: one-shot mover on switch channel {c} only ever moves once"
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
            && !map.quakes.iter().any(|q| q.channel == c)
            && !map.blasts.iter().any(|b| b.channel == c)
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
    for (i, q) in map.quakes.iter().enumerate() {
        if !channel_sourced(map, q.channel) {
            r.error(format!("quake {i}: channel {} has no source", q.channel));
        }
    }
    check_blasts(map, r);
    let exit_here = map
        .switches
        .iter()
        .any(|sw| sw.action == SwitchAction::Exit)
        || map.triggers.iter().any(|t| t.action == SwitchAction::Exit)
        || map
            .actors
            .iter()
            .any(|a| a.on_death == Some(SwitchAction::Exit));
    if !exit_here {
        r.warn("level has no exit".into());
    }
}

/// Scripted blasts: wired to a fired channel, centred inside a sector between its floor and
/// ceiling, with a positive radius and no negative damage or delay.
fn check_blasts(map: &Map, r: &mut Report) {
    for (i, b) in map.blasts.iter().enumerate() {
        if b.channel == 0 {
            r.error(format!("blast {i}: channel must not be 0"));
        } else if !channel_sourced(map, b.channel) {
            r.error(format!("blast {i}: channel {} has no source", b.channel));
        }
        let c = b.centre();
        if !c.is_finite() {
            r.error(format!("blast {i}: position must be finite"));
        } else {
            match map.find_sector(c.truncate(), None) {
                None => r.error(format!("blast {i} at {c} is outside every sector")),
                Some(s) => {
                    let sec = &map.sectors[s];
                    if !(sec.floor_z < c.z && c.z < sec.ceil_z) {
                        r.error(format!(
                            "blast {i} at {c}: z must lie between the floor and ceiling of sector {s}"
                        ));
                    }
                }
            }
        }
        if !(b.radius > 0.0 && b.radius.is_finite()) {
            r.error(format!("blast {i}: radius must be positive"));
        }
        if b.damage < 0 {
            r.error(format!("blast {i}: damage must not be negative"));
        }
        if !(b.delay >= 0.0 && b.delay.is_finite()) {
            r.error(format!("blast {i}: delay must be 0 or more seconds"));
        }
    }
}

/// The crack walls scripted blasts open, as (crack sector, the blast's channel): what
/// `explosion::sectors_in_reach` finds from each blast's centre on the level in its start pose
/// (doors and cracks closed, as `Mechanics::new` leaves them). Invalid blasts open nothing.
fn blast_openings(map: &Map) -> Vec<(SectorId, Channel)> {
    if map.blasts.is_empty() {
        return Vec::new();
    }
    let mut start = map.clone();
    Mechanics::new(&mut start);
    let mut out = Vec::new();
    for b in &map.blasts {
        let center = b.centre();
        if !(center.is_finite() && b.radius > 0.0 && b.radius.is_finite()) {
            continue;
        }
        let Some(sector) = start.find_sector(center.truncate(), None) else {
            continue;
        };
        let blast = Blast {
            center,
            sector,
            splash: SplashDef {
                radius: b.radius,
                damage: b.damage,
                self_scale: 1.0,
            },
            owner: Shooter::World,
        };
        out.extend(
            sectors_in_reach(&start, &blast)
                .into_iter()
                .map(|s| (s, b.channel)),
        );
    }
    out
}

/// Clearance a slider keeps from its track's other walls along its whole path (metres).
const SLIDE_CLEARANCE: f32 = 0.05;
/// Spacing of the poses at which a slider's path is checked (metres).
const SLIDE_STEP: f32 = 0.25;
/// A sector with a portal into the track this close to a resting slider's edge counts as next to
/// the slider: the player can step across the gap (metres, the player's radius).
const SLIDE_REACH: f32 = 0.35;

/// The track of slider `s`: the one sector that every wall of its single loop opens into, and
/// that holds the mirrored loop as one of its holes. `None` when `s` is no such island.
fn slide_track(map: &Map, s: SectorId) -> Option<SectorId> {
    let sec = &map.sectors[s];
    let [lp] = sec.loops.as_slice() else {
        return None;
    };
    let mut track = None;
    let mut twins = BTreeSet::new();
    for &w in lp {
        let (Some(t), Some(back)) = (map.walls[w].next_sector, map.walls[w].next_wall) else {
            return None;
        };
        if t == s || track.is_some_and(|x| x != t) {
            return None;
        }
        track = Some(t);
        twins.insert(back);
    }
    let t = track?;
    map.sectors[t]
        .loops
        .iter()
        .skip(1)
        .any(|hole| hole.iter().copied().collect::<BTreeSet<_>>() == twins)
        .then_some(t)
}

/// Even-odd test against one loop.
fn in_loop(map: &Map, lp: &[WallId], p: Vec2) -> bool {
    let mut inside = false;
    for &w in lp {
        let (a, b) = (map.walls[w].a, map.walls[w].b);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// Can the player operate this mover, given the keys held and channels fired?
fn operable(def: &crate::map::MoverDef, keys: KeySet, fired: &[Channel]) -> bool {
    match def.channel {
        Some(c) => fired.contains(&c),
        None => def.lock.is_none_or(|k| keys.contains(k)),
    }
}

/// Sliders: each is an island in one track, stays inside it with `SLIDE_CLEARANCE` along its
/// whole path, and carries nothing that would not move with it.
fn check_sliders(map: &Map, r: &mut Report) {
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    for (s, sec) in map.sectors.iter().enumerate() {
        let Some(MoverKind::Slide { by }) = sec.mover.map(|m| m.kind) else {
            continue;
        };
        let Some(t) = slide_track(map, s) else {
            r.error(format!(
                "sector {s}: a slider must be an island: one loop whose walls are all portals into one track sector, which holds it as a hole"
            ));
            continue;
        };
        let by = Vec2::from(by);
        let own: Vec<WallId> = sec.walls().collect();
        let twins: Vec<WallId> = own.iter().filter_map(|&w| map.walls[w].next_wall).collect();
        let others: Vec<WallId> = map.sectors[t]
            .walls()
            .filter(|w| !twins.contains(w))
            .collect();
        let holes: Vec<&Vec<WallId>> = map.sectors[t]
            .loops
            .iter()
            .skip(1)
            .filter(|h| !h.iter().any(|w| twins.contains(w)))
            .collect();
        let n = (by.length() / SLIDE_STEP).ceil().max(1.0) as usize;
        let leaves = (0..=n).map(|k| by * (k as f32 / n as f32)).find(|&off| {
            let corner = map.walls[own[0]].a + off;
            let inside = in_loop(map, &map.sectors[t].loops[0], corner)
                && !holes.iter().any(|h| in_loop(map, h, corner));
            let close = others.iter().any(|&o| {
                let (c, d) = (map.walls[o].a, map.walls[o].b);
                own.iter().any(|&w| {
                    let (a, b) = (map.walls[w].a + off, map.walls[w].b + off);
                    seg_seg_dist(a, b, c, d) < SLIDE_CLEARANCE
                }) || map.sector_contains(s, c - off)
            });
            !inside || close
        });
        if let Some(off) = leaves {
            r.error(format!(
                "sector {s}: slider leaves its track (sector {t}) or comes within {SLIDE_CLEARANCE} m of its walls {off} along its path"
            ));
        }
        let on_path =
            |p: Vec2| (0..=n).any(|k| map.sector_contains(s, p - by * (k as f32 / n as f32)));
        for (i, item) in map.items.iter().enumerate() {
            if map.find_sector(item.pos, None) == Some(s) {
                r.error(format!(
                    "item {i} is on slider sector {s}, which would leave it behind"
                ));
            }
        }
        for (i, sw) in map.switches.iter().enumerate() {
            if map.walls[sw.wall].sector == s {
                r.error(format!("switch {i} is on slider sector {s}"));
            }
        }
        for (i, p) in map.props.iter().enumerate() {
            if p.sector == t && p.corners().iter().any(|&c| on_path(c)) {
                r.error(format!(
                    "prop {i} ({:?}) stands in the path of slider sector {s}",
                    p.kind
                ));
            }
        }
        if map.find_sector(start, None) == Some(s) {
            r.error(format!("player_start is on slider sector {s}"));
        }
        for (i, l) in map.lights.iter().enumerate() {
            if map.sector_contains(s, Vec2::new(l.pos.0, l.pos.1)) {
                r.warn(format!(
                    "light {i} is inside slider sector {s}; it will not move with it"
                ));
            }
        }
    }
}

/// Extra links sliders make, per sector: at its start pose, and at its end pose once the player
/// can operate it, a slider is next to every sector (other than its track) behind a track
/// portal within `SLIDE_REACH` of its edge. Symmetric.
fn slide_contacts(map: &Map, keys: KeySet, fired: &[Channel]) -> Vec<Vec<SectorId>> {
    let mut out = vec![Vec::new(); map.sectors.len()];
    for (s, sec) in map.sectors.iter().enumerate() {
        let Some(def) = sec.mover else { continue };
        let MoverKind::Slide { by } = def.kind else {
            continue;
        };
        let Some(t) = slide_track(map, s) else {
            continue;
        };
        let mut offs = vec![Vec2::ZERO];
        if operable(&def, keys, fired) {
            offs.push(Vec2::from(by));
        }
        for off in offs {
            for w in map.sectors[t].walls() {
                let wall = &map.walls[w];
                let Some(n) = wall.passage().filter(|&n| n != s) else {
                    continue;
                };
                let near = sec.walls().any(|e| {
                    let (a, b) = (map.walls[e].a + off, map.walls[e].b + off);
                    seg_seg_dist(a, b, wall.a, wall.b) <= SLIDE_REACH
                });
                if near && !out[s].contains(&n) {
                    out[s].push(n);
                    out[n].push(s);
                }
            }
        }
    }
    out
}

/// Floor/ceiling pairs a sector can offer a player, given the keys held, the channels fired and
/// the cracks scripted blasts open (`blast_openings`).
fn poses(
    map: &Map,
    s: SectorId,
    keys: KeySet,
    fired: &[Channel],
    opened: &[(SectorId, Channel)],
) -> Vec<Pose> {
    let sec = &map.sectors[s];
    let Some(def) = sec.mover else {
        return vec![(sec.floor_z, sec.ceil_z)];
    };
    let operable = operable(&def, keys, fired);
    match (def.kind, operable) {
        (MoverKind::Door, true) => vec![(sec.floor_z, sec.ceil_z)],
        (MoverKind::Door, false) => vec![],
        (MoverKind::Lift { to }, true) => vec![(sec.floor_z, sec.ceil_z), (to, sec.ceil_z)],
        (MoverKind::Lift { .. }, false) => vec![(sec.floor_z, sec.ceil_z)],
        // Its heights never change; where it can take the player is `slide_contacts`.
        (MoverKind::Slide { .. }, _) => vec![(sec.floor_z, sec.ceil_z)],
        // Only a blast opens it, and no level may depend on a barrel or a rocket: only on a
        // scripted blast whose channel has fired.
        (MoverKind::Crack, _) if opened.iter().any(|&(c, ch)| c == s && fired.contains(&ch)) => {
            vec![(sec.floor_z, sec.ceil_z)]
        }
        (MoverKind::Crack, _) => vec![],
    }
}

fn flood(
    map: &Map,
    start: SectorId,
    keys: KeySet,
    fired: &[Channel],
    opened: &[(SectorId, Channel)],
    t: &Tuning,
) -> Vec<bool> {
    let contacts = slide_contacts(map, keys, fired);
    let mut reached = vec![false; map.sectors.len()];
    reached[start] = true;
    let mut queue = vec![start];
    while let Some(s) = queue.pop() {
        let from = poses(map, s, keys, fired, opened);
        for n in map.neighbours(s).chain(contacts[s].iter().copied()) {
            if reached[n] {
                continue;
            }
            let to = poses(map, n, keys, fired, opened);
            if from.iter().any(|&a| to.iter().any(|&b| can_cross(a, b, t))) {
                reached[n] = true;
                queue.push(n);
            }
        }
    }
    reached
}

/// Can a player holding `keys` operate `sw`? A channel switch also needs the lock key of every
/// mover listening on its channel (the same rule the runtime applies).
fn switch_usable(map: &Map, sw: &Switch, keys: KeySet) -> bool {
    sw.key.is_none_or(|k| keys.contains(k))
        && match sw.action {
            SwitchAction::Channel(c) => {
                let need = channel_required_keys(map, c);
                Key::ALL
                    .iter()
                    .all(|&k| !need.contains(k) || keys.contains(k))
            }
            SwitchAction::Exit => true,
        }
}

/// Reports unreachable items and exits; returns the final reachable-sector set (None when the
/// start is outside every sector).
fn check_reachability(map: &Map, r: &mut Report) -> Option<Vec<bool>> {
    let t = Tuning::default();
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    let Some(start) = map.find_sector(start, None) else {
        return None; // reported by check_wiring
    };
    let usable = |sw: &Switch, keys: KeySet| switch_usable(map, sw, keys);
    let mut keys = KeySet::default();
    let mut fired: Vec<Channel> = Vec::new();
    // Monotone (keys and channels only ever grow), so this always converges; the bound is a guard.
    let rounds = map.switches.len() + map.triggers.len() + map.actors.len() + Key::ALL.len() + 1;
    let opened = blast_openings(map);
    let mut reached = flood(map, start, keys, &fired, &opened, &t);
    for _ in 0..rounds {
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
        let sources = map
            .triggers
            .iter()
            .filter(|t| reached[t.sector])
            .map(|t| t.action)
            .chain(map.actors.iter().filter_map(|a| {
                let s = map.find_sector(a.pos, None)?;
                a.on_death.filter(|_| reached[s])
            }));
        for action in sources {
            if let SwitchAction::Channel(c) = action
                && !fired.contains(&c)
            {
                fired.push(c);
                changed = true;
            }
        }
        if !changed {
            break;
        }
        reached = flood(map, start, keys, &fired, &opened, &t);
    }
    for (i, item) in map.items.iter().enumerate() {
        if !map.find_sector(item.pos, None).is_some_and(|s| reached[s]) {
            r.warn(format!(
                "item {i} ({:?}) cannot be reached from the start",
                item.kind
            ));
        }
    }
    let switch_exits: Vec<&Switch> = map
        .switches
        .iter()
        .filter(|sw| sw.action == SwitchAction::Exit)
        .collect();
    let trigger_exits = || {
        map.triggers
            .iter()
            .filter(|t| t.action == SwitchAction::Exit)
    };
    let death_exits = || {
        map.actors
            .iter()
            .filter(|a| a.on_death == Some(SwitchAction::Exit))
    };
    let any_exit = !switch_exits.is_empty()
        || trigger_exits().next().is_some()
        || death_exits().next().is_some();
    let exit_reached = switch_exits
        .iter()
        .any(|sw| reached[map.walls[sw.wall].sector] && usable(sw, keys))
        || trigger_exits().any(|t| reached[t.sector])
        || death_exits().any(|a| map.find_sector(a.pos, None).is_some_and(|s| reached[s]));
    if any_exit && !exit_reached {
        r.error("no exit can be reached from the start".into());
    }
    Some(reached)
}

fn check_props(map: &Map, r: &mut Report) {
    for (i, p) in map.props.iter().enumerate() {
        let who = format!("prop {i} ({:?})", p.kind);
        if map.sectors[p.sector].mover.is_some() {
            r.error(format!("{who} is in a mover sector ({})", p.sector));
        }
        if !p
            .corners()
            .iter()
            .all(|&c| map.find_sector(c, Some(p.sector)) == Some(p.sector))
        {
            r.error(format!("{who}: box leaves sector {}", p.sector));
        }
    }
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
    for k in ActorKind::ALL.into_iter().filter(|&k| defs.enemy(k).boss) {
        if map.actors.iter().filter(|a| a.kind == k).count() > 1 {
            r.error(format!(
                "level has more than one {k:?}; at most one boss of each kind per level"
            ));
        }
    }
    for (i, a) in map.actors.iter().enumerate() {
        let at = format!("({:?}, {:?})", a.pos.x, a.pos.y);
        let who = format!("actor {i} ({:?}) at {at}", a.kind);
        let Some(s) = map.find_sector(a.pos, None) else {
            r.error(format!("{who} is outside every sector"));
            continue;
        };
        let def = defs.enemy(a.kind);
        if a.on_death.is_some() && a.skill != Difficulty::Easy {
            r.error(format!(
                "{who}: on_death actor must appear on every skill (skill: Easy)"
            ));
        }
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

/// Burrow points (`Map::burrow_points`): each must lie in a sector reached from the start that
/// is no mover and is tall enough, with the footprint of the widest burrower clear of blocking
/// walls (sizes from `Defs::builtin()`: the burrowing kinds in the level, else every burrowing
/// kind). A burrowing actor in a level with fewer than two points never burrows: a warning.
fn check_burrow_points(map: &Map, reached: Option<&[bool]>, r: &mut Report) {
    let defs = Defs::builtin();
    let burrowers: Vec<(usize, ActorKind)> = map
        .actors
        .iter()
        .enumerate()
        .filter(|(_, a)| defs.enemy(a.kind).burrows())
        .map(|(i, a)| (i, a.kind))
        .collect();
    let sizing = defs.enemies.iter().filter(|e| {
        e.burrows() && (burrowers.is_empty() || burrowers.iter().any(|b| b.1 == e.kind))
    });
    let (radius, height) = sizing.fold((0.0_f32, 0.0_f32), |(r, h), e| {
        (r.max(e.radius), h.max(e.height))
    });
    for (i, &p) in map.burrow_points.iter().enumerate() {
        let who = format!("burrow point {i} at ({:?}, {:?})", p.x, p.y);
        if !p.is_finite() {
            r.error(format!("{who}: position must be finite"));
            continue;
        }
        let Some(s) = map.find_sector(p, None) else {
            r.error(format!("{who} is outside every sector"));
            continue;
        };
        let sec = &map.sectors[s];
        if sec.mover.is_some() {
            r.error(format!("{who} is in a mover sector ({s})"));
        }
        if sec.ceil_z - sec.floor_z < height {
            r.error(format!(
                "{who}: sector {s} is too low for a {height} m burrower"
            ));
        }
        if reached.is_some_and(|re| !re[s]) {
            r.error(format!("{who} cannot be reached from the start"));
        }
        let near_wall = map.walls.iter().any(|w| {
            w.passage().is_none() && closest_point_on_segment(p, w.a, w.b).distance(p) < radius
        });
        if near_wall {
            r.error(format!("{who} is within {radius} m of a blocking wall"));
        }
    }
    if map.burrow_points.len() < 2 {
        for (i, k) in burrowers {
            r.warn(format!(
                "actor {i} ({k:?}) burrows, but the level has {} burrow points (needs 2); it never burrows",
                map.burrow_points.len()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::difficulty::Difficulty;
    use crate::fixtures::{
        combat_room, door_rooms, engine_room, lift_shaft, pillar_room, slide_yard, two_rooms,
    };
    use crate::map::ActorSpawn;
    use crate::players::PlayerId;

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
    fn skill_gated_keycard_is_an_error() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Blue))",
            "switches: [(wall: (3, 4), action: Exit)], \
             items: [(kind: Key(Blue), pos: (2.0, 3.0), skill: Normal)],",
        );
        let errs = errors(&map);
        assert!(has(&errs, "keycard") && has(&errs, "Easy"), "{errs:?}");
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

    /// The exit behind a crack (B's east wall), a channel 5 switch at `switch` and `blasts`.
    fn breach(switch: &str, blasts: &str) -> Map {
        door_rooms(
            "(kind: Crack)",
            &format!(
                "switches: [(wall: (3, 4), action: Exit), (wall: {switch}, action: Channel(5))],
                 blasts: [{blasts}],"
            ),
        )
    }

    #[test]
    fn a_scripted_blast_on_a_fired_channel_opens_the_route() {
        // From room A, 2 m from the crack's wall: in reach.
        let map = breach(
            "(7, 0)",
            "(channel: 5, at: (2.0, 2.0, 1.0), radius: 3.0, damage: 20)",
        );
        assert_eq!(validate(&map), vec![]);
        // A key behind the breach counts as reachable too.
        let mut keyed = map.clone();
        keyed.switches[0].key = Some(Key::Red);
        keyed.items.push(crate::map::Item {
            kind: ItemKind::Key(Key::Red),
            pos: Vec2::new(6.5, 2.0),
            skill: Difficulty::Easy,
        });
        assert_eq!(validate(&keyed), vec![]);
    }

    #[test]
    fn a_crack_stays_shut_without_a_scripted_blast_that_reaches_it_on_a_fired_channel() {
        // Out of reach: 1.5 m does not get to the wall 2 m away.
        let short = breach(
            "(7, 0)",
            "(channel: 5, at: (2.0, 2.0, 1.0), radius: 1.5, damage: 20)",
        );
        assert!(has(&errors(&short), "no exit can be reached"));
        // The channel's only switch is behind the crack it would open.
        let behind = breach(
            "(4, 5)",
            "(channel: 5, at: (2.0, 2.0, 1.0), radius: 3.0, damage: 20)",
        );
        assert!(has(&errors(&behind), "no exit can be reached"));
        // A barrel beside the crack is no way through: nothing may depend on one.
        let barrel = door_rooms(
            "(kind: Crack)",
            "switches: [(wall: (3, 4), action: Exit)], actors: [(kind: Barrel, pos: (3.0, 2.0))],",
        );
        assert!(has(&errors(&barrel), "no exit can be reached"));
    }

    #[test]
    fn blast_mistakes_are_reported() {
        let good = breach(
            "(7, 0)",
            "(channel: 5, at: (2.0, 2.0, 1.0), radius: 3.0, damage: 20)",
        );
        // A blast counts as a listener of its channel's switch.
        assert!(!has(&warnings(&good), "no listeners"));
        let with = |f: &dyn Fn(&mut crate::map::RawBlast)| {
            let mut m = good.clone();
            f(&mut m.blasts[0]);
            errors(&m)
        };
        assert!(has(
            &with(&|b| b.channel = 6),
            "blast 0: channel 6 has no source"
        ));
        assert!(has(
            &with(&|b| b.channel = 0),
            "blast 0: channel must not be 0"
        ));
        assert!(has(&with(&|b| b.at.0 = 30.0), "is outside every sector"));
        assert!(has(
            &with(&|b| b.at.2 = 3.5),
            "z must lie between the floor and ceiling"
        ));
        assert!(has(
            &with(&|b| b.at.2 = 0.0),
            "z must lie between the floor and ceiling"
        ));
        assert!(has(
            &with(&|b| b.at.1 = f32::NAN),
            "position must be finite"
        ));
        assert!(has(&with(&|b| b.radius = 0.0), "radius must be positive"));
        assert!(has(
            &with(&|b| b.radius = f32::INFINITY),
            "radius must be positive"
        ));
        assert!(has(
            &with(&|b| b.damage = -1),
            "damage must not be negative"
        ));
        assert!(has(&with(&|b| b.delay = -0.5), "delay must be 0 or more"));
        assert!(has(
            &with(&|b| b.delay = f32::NAN),
            "delay must be 0 or more"
        ));
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

    #[test]
    fn validate_and_runtime_agree_on_channel_keys() {
        use crate::mechanics::{Mechanics, UseOutcome, UseTarget};
        let mut map = door_rooms(
            "(kind: Door, channel: Some(1), lock: Some(Red))",
            "switches: [(wall: (7, 0), action: Channel(1))],",
        );
        map.sectors[0].mover = Some(crate::map::MoverDef {
            kind: crate::map::MoverKind::Door,
            speed: 2.5,
            lock: Some(Key::Blue),
            channel: Some(1),
            auto_return: None,
            one_shot: false,
        });
        let need = channel_required_keys(&map, 1);
        assert!(need.contains(Key::Red) && need.contains(Key::Blue) && !need.contains(Key::Yellow));
        assert_eq!(channel_required_keys(&map, 2), KeySet::default());
        for bits in 0..8u8 {
            let mut keys = KeySet::default();
            for (i, k) in Key::ALL.into_iter().enumerate() {
                if bits >> i & 1 == 1 {
                    keys.insert(k);
                }
            }
            let mut m = map.clone();
            let mut mech = Mechanics::new(&mut m);
            let runtime =
                mech.activate(&m, PlayerId(0), UseTarget::Switch(0), keys) == UseOutcome::Activated;
            assert_eq!(
                switch_usable(&map, &map.switches[0], keys),
                runtime,
                "keys {bits:03b}"
            );
        }
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
            skill: Difficulty::Easy,
            on_death: None,
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

    #[test]
    fn triggers_and_deaths_are_channel_sources() {
        assert!(!has(&errors(&engine_room("")), "channel 7"));
        let map = door_rooms(
            "(kind: Door, channel: Some(3))",
            "actors: [(kind: Grunt, pos: (2.0, 2.0), on_death: Some(Channel(3)))],
             switches: [(wall: (3, 4), action: Exit)],",
        );
        assert!(errors(&map).is_empty(), "{:?}", errors(&map));
    }

    #[test]
    fn quakes_need_a_source() {
        let mut map = engine_room("");
        map.triggers.clear();
        assert!(has(&errors(&map), "quake 0: channel 7 has no source"));
    }

    #[test]
    fn reachability_fires_trigger_channels() {
        let open = door_rooms(
            "(kind: Door, channel: Some(3))",
            "triggers: [(sector: 0, action: Channel(3))], switches: [(wall: (3, 4), action: Exit)],",
        );
        assert!(!has(&errors(&open), "exit"), "{:?}", errors(&open));
        let mut shut = open.clone();
        shut.triggers.clear();
        assert!(has(&errors(&shut), "no exit"));
    }

    #[test]
    fn on_death_exit_counts_as_an_exit() {
        let map = door_rooms(
            "(kind: Door)",
            "actors: [(kind: Grunt, pos: (6.5, 2.0), on_death: Some(Exit))],",
        );
        assert!(!has(&warnings(&map), "no exit"));
        assert!(!has(&errors(&map), "exit"), "{:?}", errors(&map));
        // The same actor behind a channel door nothing fires: the exit cannot be reached.
        let shut = door_rooms(
            "(kind: Door, channel: Some(3))",
            "actors: [(kind: Grunt, pos: (6.5, 2.0), on_death: Some(Exit))],",
        );
        assert!(
            has(&errors(&shut), "no exit can be reached"),
            "{:?}",
            errors(&shut)
        );
    }

    #[test]
    fn on_death_must_appear_on_every_skill() {
        let hard = door_rooms(
            "(kind: Door)",
            "actors: [(kind: Grunt, pos: (6.5, 2.0), skill: Hard, on_death: Some(Exit))],",
        );
        assert!(
            has(&errors(&hard), "on_death actor must appear on every skill"),
            "{:?}",
            errors(&hard)
        );
        // A plain Hard actor is fine.
        let plain = door_rooms(
            "(kind: Door)",
            "actors: [(kind: Grunt, pos: (6.5, 2.0), skill: Hard)],",
        );
        assert!(!has(&errors(&plain), "every skill"));
    }

    #[test]
    fn at_most_one_boss() {
        let one = "(kind: Boss, pos: (6.5, 2.0))";
        let map = door_rooms("(kind: Door)", &format!("actors: [{one}, {one}],"));
        assert!(has(&errors(&map), "at most one boss"));
        // One of each boss kind is fine.
        let mut map = hive("(10.0, 10.0), (25.0, 10.0)");
        map.actors.push(ActorSpawn {
            kind: ActorKind::Boss,
            pos: Vec2::new(26.0, 15.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
        });
        assert!(!has(&errors(&map), "boss"), "{:?}", errors(&map));
    }

    /// Room A x,y∈[0,20] (start (1,1)) → door x∈[20,21], y∈[8,12] → room B x∈[21,31], y∈[0,20];
    /// a sealed room C x∈[40,50], y∈[0,10]. A Hive Queen sleeps at (15, 15) in A.
    fn hive(points: &str) -> Map {
        raw(
            "(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (20.0, 8.0), (20.0, 12.0),
             (21.0, 8.0), (21.0, 12.0), (21.0, 0.0), (31.0, 0.0), (31.0, 20.0), (21.0, 20.0),
             (40.0, 0.0), (50.0, 0.0), (50.0, 10.0), (40.0, 10.0)",
            &format!(
                "({SQ}, loops: [[0, 1, 4, 5, 2, 3]]), ({SQ}, loops: [[4, 6, 7, 5]], mover: Some((kind: Door))),
                 ({SQ}, loops: [[8, 9, 10, 11, 7, 6]]), ({SQ}, loops: [[12, 13, 14, 15]])"
            ),
            &format!(
                "actors: [(kind: HiveQueen, pos: (15.0, 15.0))], burrow_points: [{points}],"
            ),
        )
    }

    #[test]
    fn burrow_points_in_open_reachable_floor_are_clean() {
        let map = hive("(10.0, 10.0), (25.0, 10.0), (5.0, 15.0)");
        assert!(!has(&errors(&map), "burrow"), "{:?}", errors(&map));
        assert!(!has(&warnings(&map), "burrow"), "{:?}", warnings(&map));
    }

    #[test]
    fn burrow_point_mistakes_are_errors() {
        let map = hive("(10.0, 10.0), (20.5, 10.0), (0.5, 10.0), (45.0, 5.0), (100.0, 100.0)");
        let e = errors(&map);
        assert!(!has(&e, "burrow point 0"), "{e:?}");
        assert!(
            has(
                &e,
                "burrow point 1 at (20.5, 10.0) is in a mover sector (1)"
            ),
            "{e:?}"
        );
        assert!(
            has(
                &e,
                "burrow point 2 at (0.5, 10.0) is within 1.4 m of a blocking wall"
            ),
            "{e:?}"
        );
        assert!(
            has(
                &e,
                "burrow point 3 at (45.0, 5.0) cannot be reached from the start"
            ),
            "{e:?}"
        );
        assert!(
            has(
                &e,
                "burrow point 4 at (100.0, 100.0) is outside every sector"
            ),
            "{e:?}"
        );
        // The footprint is the Queen's, so a point 1.5 m off a wall fits.
        assert!(!has(&errors(&hive("(1.5, 10.0), (10.0, 10.0)")), "burrow"));
    }

    #[test]
    fn a_queen_with_fewer_than_two_points_warns() {
        for points in ["", "(10.0, 10.0)"] {
            let w = warnings(&hive(points));
            assert!(has(&w, "actor 0 (HiveQueen) burrows"), "{w:?}");
        }
        assert!(!has(
            &warnings(&hive("(10.0, 10.0), (5.0, 5.0)")),
            "burrows"
        ));
        // Points without a burrower are checked with the widest burrowing kind's footprint.
        let mut map = hive("(0.5, 10.0), (10.0, 10.0)");
        map.actors.clear();
        assert!(has(&errors(&map), "within 1.4 m"));
    }

    #[test]
    fn props_stay_inside_a_still_sector() {
        let mut map = engine_room("");
        map.props[2].pos.x = 13.5; // the pool table reaches into the pit
        assert!(has(
            &errors(&map),
            "prop 2 (PoolTable): box leaves sector 4"
        ));
        let mut map = engine_room("");
        map.props[0].pos = Vec2::new(8.0, 2.0);
        map.props[0].sector = 2;
        assert!(has(
            &errors(&map),
            "prop 0 (Toilet) is in a mover sector (2)"
        ));
    }

    #[test]
    fn one_shot_mover_on_a_switch_channel_warns() {
        let map = door_rooms(
            "(kind: Door, channel: Some(1), one_shot: true)",
            "switches: [(wall: (7, 0), action: Channel(1))],",
        );
        assert!(has(&warnings(&map), "one-shot"));
    }

    const PLATFORM: &str = "(kind: Slide(by: (7.8, 0.0)), speed: 2.0)";
    /// An exit on ledge B's east wall and a medkit on the ledge.
    const LEDGE: &str =
        "switches: [(wall: (4, 5), action: Exit)], items: [(kind: Medkit, pos: (16.0, 2.0))],";

    #[test]
    fn ledge_reached_only_through_a_slider_end_pose() {
        assert_eq!(validate(&slide_yard(PLATFORM, LEDGE)), vec![]);
        // Called by a switch on the ledge itself: the slider never leaves, so neither does
        // anyone reach the ledge.
        let stuck = slide_yard(
            "(kind: Slide(by: (7.8, 0.0)), speed: 2.0, channel: Some(9))",
            "switches: [(wall: (4, 5), action: Exit), (wall: (5, 6), action: Channel(9))], \
             items: [(kind: Medkit, pos: (16.0, 2.0))],",
        );
        assert!(
            has(&errors(&stuck), "no exit can be reached"),
            "{:?}",
            errors(&stuck)
        );
        assert!(has(&warnings(&stuck), "item 0 (Medkit) cannot be reached"));
        // Stopping short of the ledge (1 m off) does not reach it either.
        let short = slide_yard("(kind: Slide(by: (6.8, 0.0)), speed: 2.0)", LEDGE);
        assert!(has(&errors(&short), "no exit can be reached"));
    }

    #[test]
    fn slider_must_be_an_island() {
        let map = door_rooms("(kind: Slide(by: (0.0, 1.0)))", "");
        assert!(
            has(&errors(&map), "sector 1: a slider must be an island"),
            "{:?}",
            errors(&map)
        );
    }

    #[test]
    fn slider_path_must_stay_in_its_track() {
        for by in ["(7.9, 0.0)", "(8.5, 0.0)", "(0.0, 1.5)", "(-0.2, 0.0)"] {
            let map = slide_yard(&format!("(kind: Slide(by: {by}))"), LEDGE);
            assert!(
                has(
                    &errors(&map),
                    "sector 2: slider leaves its track (sector 1)"
                ),
                "{by}: {:?}",
                errors(&map)
            );
        }
        // A pillar standing in the way.
        let pillar = Map::from_ron(
            r#"(name: "t", materials: ["m"],
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0),
                       (1.0, 1.0), (2.0, 1.0), (2.0, 3.0), (1.0, 3.0),
                       (6.0, 1.9), (6.0, 2.1), (6.2, 2.1), (6.2, 1.9)],
            sectors: [
                (loops: [[0, 1, 2, 3], [4, 7, 6, 5], [8, 9, 10, 11]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
                (loops: [[4, 5, 6, 7]], floor_z: 2.9, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0,
                 mover: Some((kind: Slide(by: (7.0, 0.0))))),
            ],
            player_start: (pos: (0.5, 0.5), angle_deg: 0.0))"#,
        )
        .unwrap();
        assert!(
            has(&errors(&pillar), "slider leaves its track"),
            "{:?}",
            errors(&pillar)
        );
    }

    #[test]
    fn nothing_may_stand_on_or_in_the_way_of_a_slider() {
        let errs = errors(&slide_yard(
            PLATFORM,
            "items: [(kind: Medkit, pos: (5.0, 2.0))], \
             props: [(kind: PoolTable, pos: (9.0, 2.0), angle_deg: 0.0)], \
             switches: [(wall: (10, 11), action: Exit)], \
             lights: [(pos: (5.0, 2.0, 3.0), color: (1.0, 1.0, 1.0), intensity: 1000.0, range: 5.0)],",
        ));
        assert!(has(&errs, "item 0 is on slider sector 2"), "{errs:?}");
        assert!(has(&errs, "switch 0 is on slider sector 2"), "{errs:?}");
        assert!(
            has(
                &errs,
                "prop 0 (PoolTable) stands in the path of slider sector 2"
            ),
            "{errs:?}"
        );
        let mut map = slide_yard(PLATFORM, LEDGE);
        map.player_start.pos = (5.0, 2.0);
        assert!(has(&errors(&map), "player_start is on slider sector 2"));
        let lit = slide_yard(
            PLATFORM,
            "lights: [(pos: (5.0, 2.0, 3.0), color: (1.0, 1.0, 1.0), intensity: 1000.0, range: 5.0)],",
        );
        assert!(has(&warnings(&lit), "light 0 is inside slider sector 2"));
    }
}
