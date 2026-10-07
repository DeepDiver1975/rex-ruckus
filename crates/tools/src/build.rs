//! `rr-tools build`: compiles a level source (`levels/src/*.ron`), where sectors are coordinate
//! shapes and references are by name, into the game's level RON (`assets/levels/*.ron`).
//! Shared walls need no matching vertex indices: vertices are merged on a 1 mm grid, every edge
//! is split at vertices lying on it (T-junctions), and loops are re-wound (outer CCW, holes CW).

use rr_core::hazard::Hazard;
use rr_core::map::{
    KNOWN_MATERIALS, Key, MoverDef, PlayerStart, RawActor, RawItem, RawLight, RawQuake,
    SwitchAction,
};
use rr_core::props::RawProp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;

pub type Pt = (f32, f32);

#[derive(Debug, Deserialize)]
pub struct Defaults {
    pub floor_z: f32,
    pub ceil_z: f32,
    pub floor: String,
    pub ceil: String,
    pub wall: String,
}

#[derive(Debug, Deserialize)]
pub struct SourceSector {
    pub id: String,
    #[serde(default)]
    pub rect: Option<(f32, f32, f32, f32)>,
    #[serde(default)]
    pub poly: Option<Vec<Pt>>,
    #[serde(default)]
    pub holes: Vec<Vec<Pt>>,
    #[serde(default)]
    pub floor_z: Option<f32>,
    #[serde(default)]
    pub ceil_z: Option<f32>,
    #[serde(default)]
    pub floor: Option<String>,
    #[serde(default)]
    pub ceil: Option<String>,
    #[serde(default)]
    pub wall: Option<String>,
    #[serde(default)]
    pub face: Option<String>,
    #[serde(default)]
    pub mover: Option<MoverDef>,
    #[serde(default)]
    pub hazard: Option<Hazard>,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

/// A straight flight: `rect` cut into `steps` strips along `dir` (the climbing direction),
/// floors stepping evenly from `from_z` (first strip) to `to_z` (last). Sectors `id#0..`.
#[derive(Debug, Deserialize)]
pub struct Stairs {
    pub id: String,
    pub rect: (f32, f32, f32, f32),
    pub dir: Dir,
    pub steps: u32,
    pub from_z: f32,
    pub to_z: f32,
    #[serde(default)]
    pub ceil_z: Option<f32>,
    #[serde(default)]
    pub floor: Option<String>,
    #[serde(default)]
    pub ceil: Option<String>,
    #[serde(default)]
    pub wall: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SourceSwitch {
    /// The wall's two end points; the switch faces `sector`, which must own that edge.
    pub wall: (Pt, Pt),
    pub sector: String,
    pub action: SwitchAction,
    #[serde(default)]
    pub key: Option<Key>,
}

#[derive(Debug, Deserialize)]
pub struct SourceTrigger {
    pub sector: String,
    pub action: SwitchAction,
    #[serde(default = "yes")]
    pub once: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SourceLevel {
    pub name: String,
    pub defaults: Defaults,
    pub sectors: Vec<SourceSector>,
    #[serde(default)]
    pub stairs: Vec<Stairs>,
    pub player_start: PlayerStart,
    #[serde(default)]
    pub lights: Vec<RawLight>,
    #[serde(default)]
    pub switches: Vec<SourceSwitch>,
    #[serde(default)]
    pub items: Vec<RawItem>,
    #[serde(default)]
    pub actors: Vec<RawActor>,
    /// Glass panes: the end points of one side of a portal.
    #[serde(default)]
    pub glass: Vec<(Pt, Pt)>,
    #[serde(default)]
    pub music: Option<String>,
    #[serde(default)]
    pub triggers: Vec<SourceTrigger>,
    #[serde(default)]
    pub quakes: Vec<RawQuake>,
    #[serde(default)]
    pub props: Vec<RawProp>,
}

/// Millimetre grid key.
type Key2 = (i64, i64);

fn snap(p: Pt) -> Key2 {
    ((p.0 * 1000.0).round() as i64, (p.1 * 1000.0).round() as i64)
}

fn signed_area2(lp: &[Key2]) -> i128 {
    (0..lp.len())
        .map(|i| {
            let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
            a.0 as i128 * b.1 as i128 - b.0 as i128 * a.1 as i128
        })
        .sum()
}

/// `v` lies strictly between `a` and `b` on the segment (within 0.5 mm of the line): its
/// position along it, 0..1.
fn on_segment(a: Key2, b: Key2, v: Key2) -> Option<f64> {
    if v == a || v == b {
        return None;
    }
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let len2 = dx * dx + dy * dy;
    let (wx, wy) = ((v.0 - a.0) as f64, (v.1 - a.1) as f64);
    let t = (wx * dx + wy * dy) / len2;
    let cross = (dx * wy - dy * wx).abs() / len2.sqrt();
    (t > 0.0 && t < 1.0 && cross <= 0.5).then_some(t)
}

fn rect_loop(r: (f32, f32, f32, f32)) -> Vec<Pt> {
    let (x0, y0, x1, y1) = (r.0.min(r.2), r.1.min(r.3), r.0.max(r.2), r.1.max(r.3));
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// Snaps, drops consecutive duplicates, and winds: outer CCW, holes CW.
fn clean_loop(id: &str, pts: &[Pt], outer: bool) -> Result<Vec<Key2>, String> {
    let mut lp: Vec<Key2> = Vec::new();
    for &p in pts {
        let k = snap(p);
        if lp.last() != Some(&k) {
            lp.push(k);
        }
    }
    while lp.len() > 1 && lp.first() == lp.last() {
        lp.pop();
    }
    if lp.len() < 3 || signed_area2(&lp) == 0 {
        return Err(format!(
            "sector \"{id}\": loop has fewer than 3 distinct points after 1 mm snapping"
        ));
    }
    if (signed_area2(&lp) > 0) != outer {
        lp.reverse();
    }
    Ok(lp)
}

fn fmt_key(k: Key2) -> String {
    format!("{:?}, {:?}", k.0 as f32 / 1000.0, k.1 as f32 / 1000.0)
}

fn ron<T: Serialize>(v: &T) -> Result<String, String> {
    ron::to_string(v).map_err(|e| format!("emit: {e}"))
}

/// The geometry after merging and splitting: vertex keys and per-sector vertex-index loops.
struct Geometry {
    verts: Vec<Key2>,
    index: HashMap<Key2, usize>,
    loops: Vec<Vec<Vec<usize>>>,
}

impl Geometry {
    fn new(level: &SourceLevel) -> Result<Geometry, String> {
        let mut verts = Vec::new();
        let mut index = HashMap::new();
        let mut raw_loops = Vec::new();
        for s in &level.sectors {
            let outer = match (&s.rect, &s.poly) {
                (Some(r), None) => rect_loop(*r),
                (None, Some(p)) => p.clone(),
                _ => {
                    return Err(format!(
                        "sector \"{}\": give exactly one of rect or poly",
                        s.id
                    ));
                }
            };
            let mut loops = vec![clean_loop(&s.id, &outer, true)?];
            for h in &s.holes {
                loops.push(clean_loop(&s.id, h, false)?);
            }
            let loops: Vec<Vec<usize>> = loops
                .iter()
                .map(|lp| {
                    lp.iter()
                        .map(|&k| {
                            *index.entry(k).or_insert_with(|| {
                                verts.push(k);
                                verts.len() - 1
                            })
                        })
                        .collect()
                })
                .collect();
            raw_loops.push(loops);
        }
        // T-junctions: every edge is split at the vertices lying on it, in order along it.
        let loops: Vec<Vec<Vec<usize>>> = raw_loops
            .iter()
            .map(|sector| {
                sector
                    .iter()
                    .map(|lp| {
                        let mut out = Vec::with_capacity(lp.len());
                        for i in 0..lp.len() {
                            let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
                            out.push(a);
                            out.extend(split_points(&verts, verts[a], verts[b]));
                        }
                        out
                    })
                    .collect()
            })
            .collect();
        for (s, sector) in level.sectors.iter().zip(&loops) {
            check_folds(&s.id, sector, &verts)?;
        }
        Ok(Geometry {
            verts,
            index,
            loops,
        })
    }

    /// The edge between `a` and `b` (either way round) in the given sectors' loops, in loop
    /// order. `what` names the reference in errors.
    fn find_edge(
        &self,
        what: &str,
        (a, b): (Pt, Pt),
        sectors: impl Iterator<Item = usize> + Clone,
        place: &str,
    ) -> Result<(usize, usize), String> {
        let (ka, kb) = (snap(a), snap(b));
        let (Some(&ia), Some(&ib)) = (self.index.get(&ka), self.index.get(&kb)) else {
            return Err(format!(
                "{what}: no wall ({})–({}) {place}",
                fmt_key(ka),
                fmt_key(kb)
            ));
        };
        for s in sectors {
            for lp in &self.loops[s] {
                for i in 0..lp.len() {
                    let e = (lp[i], lp[(i + 1) % lp.len()]);
                    if e == (ia, ib) || e == (ib, ia) {
                        return Ok(e);
                    }
                }
            }
        }
        if let Some(&v) = split_points(&self.verts, ka, kb).first() {
            return Err(format!(
                "{what}: wall is split at {}; name one sub-wall",
                fmt_key(self.verts[v])
            ));
        }
        Err(format!(
            "{what}: no wall ({})–({}) {place}",
            fmt_key(ka),
            fmt_key(kb)
        ))
    }
}

/// Rejects a sector whose split loops run an edge both ways (a hole sharing an edge with the
/// outer loop, or a zero-width spike), which would link the sector to itself, and a loop that
/// passes through one vertex twice. A hole touching the outer loop at a single corner is fine.
fn check_folds(id: &str, loops: &[Vec<usize>], verts: &[Key2]) -> Result<(), String> {
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    for (l, lp) in loops.iter().enumerate() {
        for i in 0..lp.len() {
            edges.insert((lp[i], lp[(i + 1) % lp.len()]), l);
        }
    }
    for (l, lp) in loops.iter().enumerate() {
        for i in 0..lp.len() {
            let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
            if let Some(&other) = edges.get(&(b, a)) {
                let what = if other == l {
                    "edge folds back on itself"
                } else {
                    "hole shares an edge with the outer loop"
                };
                return Err(format!(
                    "sector \"{id}\": {what} at ({})–({})",
                    fmt_key(verts[a]),
                    fmt_key(verts[b])
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(&v) = lp.iter().find(|&&v| !seen.insert(v)) {
            return Err(format!(
                "sector \"{id}\": loop passes through ({}) twice",
                fmt_key(verts[v])
            ));
        }
    }
    Ok(())
}

/// Indices of the vertices strictly inside segment `a`–`b`, ordered from `a`.
fn split_points(verts: &[Key2], a: Key2, b: Key2) -> Vec<usize> {
    let mut hits: Vec<(f64, usize)> = verts
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| on_segment(a, b, v).map(|t| (t, i)))
        .collect();
    hits.sort_by(|x, y| x.0.total_cmp(&y.0));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The leading `//` comment lines of the source, kept as the output's header.
fn header(src: &str) -> Vec<&str> {
    src.lines()
        .take_while(|l| l.trim_start().starts_with("//"))
        .collect()
}

/// Parses with `implicit_some`, so optional fields read `rect: (…)` rather than `Some((…))`
/// (an explicit `Some(…)` still parses).
fn parse(src: &str) -> Result<SourceLevel, String> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str::<SourceLevel>(src)
        .map_err(|e| format!("parse: {e}"))
}

/// Compiles a level source into level RON; the "Built by" line names
/// `levels/src/<name>.ron`, with the level name lowercased and spaces turned into `_`.
pub fn build_source(src: &str) -> Result<String, String> {
    let stem = parse(src)?.name.to_lowercase().replace(' ', "_");
    build_source_named(src, &stem)
}

/// Largest riser a flight may have, in metres.
const MAX_RISER: f32 = 0.55;

/// Appends each flight's strips to `level.sectors` as `id#0..`.
fn expand_stairs(level: &mut SourceLevel) -> Result<(), String> {
    for st in std::mem::take(&mut level.stairs) {
        if st.steps == 0 {
            return Err(format!("stairs \"{}\": steps must be at least 1", st.id));
        }
        let n = st.steps as f32;
        let riser = if st.steps > 1 {
            (st.to_z - st.from_z).abs() / (n - 1.0)
        } else {
            0.0
        };
        if riser > MAX_RISER {
            return Err(format!(
                "stairs \"{}\": riser {riser:.2} m exceeds {MAX_RISER} m",
                st.id
            ));
        }
        let (x0, y0, x1, y1) = st.rect;
        for i in 0..st.steps {
            let (a, b) = (i as f32 / n, (i + 1) as f32 / n);
            let lerp = |lo: f32, hi: f32, t: f32| lo + (hi - lo) * t;
            let rect = match st.dir {
                Dir::E => (lerp(x0, x1, a), y0, lerp(x0, x1, b), y1),
                Dir::W => (lerp(x1, x0, b), y0, lerp(x1, x0, a), y1),
                Dir::N => (x0, lerp(y0, y1, a), x1, lerp(y0, y1, b)),
                Dir::S => (x0, lerp(y1, y0, b), x1, lerp(y1, y0, a)),
            };
            let floor_z = if st.steps > 1 {
                st.from_z + (st.to_z - st.from_z) * i as f32 / (n - 1.0)
            } else {
                st.from_z
            };
            level.sectors.push(SourceSector {
                id: format!("{}#{i}", st.id),
                rect: Some(rect),
                poly: None,
                holes: Vec::new(),
                floor_z: Some(floor_z),
                ceil_z: st.ceil_z,
                floor: st.floor.clone(),
                ceil: st.ceil.clone(),
                wall: st.wall.clone(),
                face: None,
                mover: None,
                hazard: None,
                secret: false,
            });
        }
    }
    Ok(())
}

/// Compiles a level source into level RON; `stem` is the source file's stem, named in the
/// "Built by" line.
pub fn build_source_named(src: &str, stem: &str) -> Result<String, String> {
    let mut level = parse(src)?;
    expand_stairs(&mut level)?;
    let mut ids: HashMap<&str, usize> = HashMap::new();
    for (i, s) in level.sectors.iter().enumerate() {
        if ids.insert(&s.id, i).is_some() {
            return Err(format!("duplicate sector id \"{}\"", s.id));
        }
    }
    let geo = Geometry::new(&level)?;

    // Materials, in first-use order.
    let mut materials: Vec<String> = Vec::new();
    let mut mat = |id: &str, name: &str| -> Result<usize, String> {
        if !KNOWN_MATERIALS.contains(&name) {
            return Err(format!("sector \"{id}\": unknown material \"{name}\""));
        }
        Ok(match materials.iter().position(|m| m == name) {
            Some(i) => i,
            None => {
                materials.push(name.to_string());
                materials.len() - 1
            }
        })
    };
    let d = &level.defaults;
    let mut sector_mats = Vec::new();
    for s in &level.sectors {
        let floor = mat(&s.id, s.floor.as_deref().unwrap_or(&d.floor))?;
        let ceil = mat(&s.id, s.ceil.as_deref().unwrap_or(&d.ceil))?;
        let wall = mat(&s.id, s.wall.as_deref().unwrap_or(&d.wall))?;
        let face = s.face.as_deref().map(|f| mat(&s.id, f)).transpose()?;
        sector_mats.push((floor, ceil, wall, face));
    }

    let mut switches = Vec::new();
    for (i, sw) in level.switches.iter().enumerate() {
        let what = format!("switch {i}");
        let &s = ids
            .get(sw.sector.as_str())
            .ok_or_else(|| format!("{what}: unknown sector \"{}\"", sw.sector))?;
        let place = format!("in sector \"{}\"", sw.sector);
        switches.push(geo.find_edge(&what, sw.wall, std::iter::once(s), &place)?);
    }
    let mut glass = Vec::new();
    for (i, &g) in level.glass.iter().enumerate() {
        glass.push(geo.find_edge(
            &format!("glass {i}"),
            g,
            0..level.sectors.len(),
            "in any sector",
        )?);
    }
    let mut triggers = Vec::new();
    for (i, t) in level.triggers.iter().enumerate() {
        let &s = ids
            .get(t.sector.as_str())
            .ok_or_else(|| format!("trigger {i}: unknown sector \"{}\"", t.sector))?;
        triggers.push(s);
    }

    emit(
        &level,
        stem,
        src,
        &geo,
        &materials,
        &sector_mats,
        &switches,
        &glass,
        &triggers,
    )
    .map_err(|e| e.to_string())
}

type SectorMats = (usize, usize, usize, Option<usize>);

#[allow(clippy::too_many_arguments)]
fn emit(
    level: &SourceLevel,
    stem: &str,
    src: &str,
    geo: &Geometry,
    materials: &[String],
    sector_mats: &[SectorMats],
    switches: &[(usize, usize)],
    glass: &[(usize, usize)],
    triggers: &[usize],
) -> Result<String, String> {
    let w = |e: std::fmt::Error| e.to_string();
    let mut o = String::new();
    for line in header(src) {
        writeln!(o, "{line}").map_err(w)?;
    }
    writeln!(
        o,
        "// Built by `rr-tools build` from levels/src/{stem}.ron. Do not edit; edit the source and rebuild."
    )
    .map_err(w)?;
    writeln!(o, "(").map_err(w)?;
    writeln!(o, "    name: {},", ron(&level.name)?).map_err(w)?;
    writeln!(o, "    materials: {},", list(materials.iter().map(ron))?).map_err(w)?;
    writeln!(o, "    vertices: [").map_err(w)?;
    for (i, &k) in geo.verts.iter().enumerate() {
        writeln!(o, "        ({}), // {i}", fmt_key(k)).map_err(w)?;
    }
    writeln!(o, "    ],").map_err(w)?;
    writeln!(o, "    sectors: [").map_err(w)?;
    for (i, s) in level.sectors.iter().enumerate() {
        let (floor, ceil, wall, face) = sector_mats[i];
        writeln!(o, "        // {i} {}", s.id).map_err(w)?;
        writeln!(
            o,
            "        (loops: {:?}, floor_z: {:?}, ceil_z: {:?}, floor_mat: {floor}, ceil_mat: {ceil}, \
             wall_mat: {wall}, face_mat: {face:?}, mover: {}, secret: {}, hazard: {}),",
            geo.loops[i],
            s.floor_z.unwrap_or(level.defaults.floor_z),
            s.ceil_z.unwrap_or(level.defaults.ceil_z),
            ron(&s.mover)?,
            s.secret,
            ron(&s.hazard)?,
        )
        .map_err(w)?;
    }
    writeln!(o, "    ],").map_err(w)?;
    writeln!(o, "    player_start: {},", ron(&level.player_start)?).map_err(w)?;
    writeln!(o, "    lights: {},", list(level.lights.iter().map(ron))?).map_err(w)?;
    let sw = level.switches.iter().zip(switches).map(|(s, (a, b))| {
        Ok(format!(
            "(wall: ({a}, {b}), action: {}, key: {})",
            ron(&s.action)?,
            ron(&s.key)?
        ))
    });
    writeln!(o, "    switches: {},", list(sw)?).map_err(w)?;
    writeln!(o, "    items: {},", list(level.items.iter().map(ron))?).map_err(w)?;
    writeln!(o, "    actors: {},", list(level.actors.iter().map(ron))?).map_err(w)?;
    let gl = glass.iter().map(|(a, b)| Ok(format!("({a}, {b})")));
    writeln!(o, "    glass: {},", list(gl)?).map_err(w)?;
    writeln!(o, "    music: {},", ron(&level.music)?).map_err(w)?;
    let tr = level.triggers.iter().zip(triggers).map(|(t, s)| {
        Ok(format!(
            "(sector: {s}, action: {}, once: {})",
            ron(&t.action)?,
            t.once
        ))
    });
    writeln!(o, "    triggers: {},", list(tr)?).map_err(w)?;
    writeln!(o, "    quakes: {},", list(level.quakes.iter().map(ron))?).map_err(w)?;
    writeln!(o, "    props: {},", list(level.props.iter().map(ron))?).map_err(w)?;
    writeln!(o, ")").map_err(w)?;
    Ok(o)
}

/// `[a, b, …]` of already-formatted items.
fn list(items: impl Iterator<Item = Result<String, String>>) -> Result<String, String> {
    Ok(format!(
        "[{}]",
        items.collect::<Result<Vec<_>, _>>()?.join(", ")
    ))
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
