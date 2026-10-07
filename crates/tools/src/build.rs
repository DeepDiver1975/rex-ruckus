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
        let loops = raw_loops
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

/// Compiles a level source into level RON; `stem` is the source file's stem, named in the
/// "Built by" line.
pub fn build_source_named(src: &str, stem: &str) -> Result<String, String> {
    let level = parse(src)?;
    let mut ids: HashMap<&str, usize> = HashMap::new();
    for (i, s) in level.sectors.iter().enumerate() {
        if ids.insert(&s.id, i).is_some() {
            return Err(format!("duplicate sector id \"{}\"", s.id));
        }
    }
    if !level.stairs.is_empty() {
        return Err("stairs are not supported yet".into());
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
mod tests {
    use super::*;
    use rr_core::map::Map;

    const DEFAULTS: &str =
        r#"defaults: (floor_z: 0.0, ceil_z: 3.0, floor: "concrete", ceil: "metal", wall: "brick")"#;

    fn level(body: &str) -> String {
        format!(
            r#"(name: "t", {DEFAULTS}, player_start: (pos: (1.0, 1.0), angle_deg: 0.0), {body})"#
        )
    }
    fn built(body: &str) -> Map {
        let out = build_source(&level(body)).unwrap_or_else(|e| panic!("{e}"));
        Map::from_ron(&out).unwrap_or_else(|e| panic!("{e}\n{out}"))
    }
    fn portals(map: &Map) -> usize {
        map.walls.iter().filter(|w| w.passage().is_some()).count()
    }
    /// Distinct wall end points: the built map keeps no vertex list of its own.
    fn vertex_count(map: &Map) -> usize {
        let mut pts: Vec<(i64, i64)> = map
            .walls
            .iter()
            .flat_map(|w| [w.a, w.b])
            .map(|p| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64))
            .collect();
        pts.sort();
        pts.dedup();
        pts.len()
    }

    #[test]
    fn shared_edge_merges_into_a_portal_pair() {
        let m = built(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0)), (id: "b", rect: (4.0, 0.0, 8.0, 4.0))]"#,
        );
        assert_eq!(vertex_count(&m), 6);
        assert_eq!(portals(&m), 2);
    }

    #[test]
    fn t_junction_is_split_so_the_small_room_links() {
        // A 2 m room against the middle of a 10 m wall.
        let body = r#"sectors: [(id: "hall", rect: (0.0, 0.0, 10.0, 4.0)), (id: "nook", rect: (4.0, 4.0, 6.0, 6.0))]"#;
        let m = built(body);
        assert_eq!(portals(&m), 2);
        let (out, ok) = crate::validate_source("t", &build_source(&level(body)).unwrap());
        assert!(ok, "{out}");
    }

    #[test]
    fn split_ignores_vertices_off_the_segment() {
        // (12, 0) is collinear with the hall's south wall but beyond its end: no split.
        let m = built(
            r#"sectors: [(id: "hall", rect: (0.0, 0.0, 10.0, 4.0)), (id: "far", rect: (12.0, 0.0, 14.0, 2.0))]"#,
        );
        assert_eq!(m.sectors[0].loops[0].len(), 4);
    }

    #[test]
    fn touching_hole_corner_is_split_cleanly() {
        // A diamond pillar hole whose bottom corner touches the room's south wall.
        let m = built(
            r#"sectors: [(id: "room", rect: (0.0, 0.0, 10.0, 10.0), holes: [[(5.0, 0.0), (6.0, 1.0), (5.0, 2.0), (4.0, 1.0)]])]"#,
        );
        assert_eq!(
            m.sectors[0].loops[0].len(),
            5,
            "south wall split at the corner"
        );
        assert!(
            m.walls.iter().all(|w| (w.b - w.a).length() > 1e-3),
            "no zero-length walls"
        );
    }

    #[test]
    fn clockwise_outer_and_ccw_hole_are_reoriented() {
        let src = level(
            r#"sectors: [(id: "r", poly: [(0.0, 0.0), (0.0, 6.0), (6.0, 6.0), (6.0, 0.0)],
            holes: [[(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)]])]"#,
        );
        let (out, ok) = crate::validate_source("t", &build_source(&src).unwrap());
        assert!(ok, "{out}");
    }

    #[test]
    fn near_duplicate_points_merge_and_degenerate_loop_errors() {
        let m = built(
            r#"sectors: [(id: "a", poly: [(0.0, 0.0), (4.0, 0.0), (4.0004, 0.0), (4.0, 4.0), (0.0, 4.0)])]"#,
        );
        assert_eq!(m.sectors[0].loops[0].len(), 4);
        let e = build_source(&level(
            r#"sectors: [(id: "sliver", poly: [(0.0, 0.0), (4.0, 0.0), (4.0, 0.0004)])]"#,
        ))
        .unwrap_err();
        assert!(e.contains("sliver"), "{e}");
    }

    #[test]
    fn materials_are_named_and_collected_in_first_use_order() {
        let out = build_source(&level(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0), floor: "carpet")]"#,
        ))
        .unwrap();
        assert!(
            out.contains(r#"materials: ["carpet", "metal", "brick"]"#),
            "{out}"
        );
        let e = build_source(&level(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0), floor: "plaid")]"#,
        ))
        .unwrap_err();
        assert!(e.contains("plaid") && e.contains("\"a\""), "{e}");
    }

    #[test]
    fn switch_wall_by_coordinates_faces_the_named_sector() {
        let m = built(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0))],
            switches: [(wall: ((4.0, 0.0), (4.0, 4.0)), sector: "a", action: Exit)]"#,
        );
        let w = &m.walls[m.switches[0].wall];
        assert_eq!(w.sector, 0);
        assert!((w.a.x - 4.0).abs() < 1e-6 && (w.b.x - 4.0).abs() < 1e-6);
    }

    #[test]
    fn switch_on_split_wall_is_an_error() {
        let e = build_source(&level(
            r#"sectors: [(id: "hall", rect: (0.0, 0.0, 10.0, 4.0)), (id: "nook", rect: (4.0, 4.0, 6.0, 6.0))],
            switches: [(wall: ((10.0, 4.0), (0.0, 4.0)), sector: "hall", action: Exit)]"#,
        ))
        .unwrap_err();
        assert!(e.contains("switch 0") && e.contains("split"), "{e}");
    }

    #[test]
    fn triggers_name_sectors_and_unknown_ids_error() {
        let m = built(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0)), (id: "pad", rect: (4.0, 0.0, 6.0, 4.0))],
            triggers: [(sector: "pad", action: Exit)]"#,
        );
        assert_eq!(m.triggers[0].sector, 1);
        let e = build_source(&level(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0))], triggers: [(sector: "nope", action: Exit)]"#,
        ))
        .unwrap_err();
        assert!(e.contains("nope"), "{e}");
    }

    #[test]
    fn duplicate_sector_id_is_an_error() {
        let e = build_source(&level(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0)), (id: "a", rect: (4.0, 0.0, 8.0, 4.0))]"#,
        ))
        .unwrap_err();
        assert!(e.contains("duplicate") && e.contains("\"a\""), "{e}");
    }

    const PASSTHROUGH: &str = r#"sectors: [
            (id: "a", rect: (0.0, 0.0, 4.0, 4.0)),
            (id: "lift", rect: (4.0, 0.0, 6.0, 4.0), face: "door",
             mover: (kind: Lift(to: 1.5), lock: Red, channel: 3), secret: true),
            (id: "pit", rect: (0.0, 4.0, 4.0, 6.0), floor: "slime",
             hazard: (damage: 5, interval: 0.5, kind: Slime)),
        ],
        lights: [(pos: (1.0, 1.0, 2.5), color: (1.0, 0.5, 0.2), intensity: 800.0, range: 8.0, shadows: false)],
        switches: [(wall: ((0.0, 4.0), (0.0, 0.0)), sector: "a", action: Channel(3), key: Blue)],
        items: [(kind: Key(Red), pos: (2.0, 2.0)), (kind: Medkit, pos: (3.0, 3.0), skill: Hard)],
        actors: [(kind: Grunt, pos: (3.0, 1.0), angle_deg: 90.0, on_death: Exit)],
        glass: [((4.0, 4.0), (4.0, 0.0))],
        music: "theme",
        quakes: [(channel: 3, duration: 2.0, strength: 0.5)],
        props: [(kind: Toilet, pos: (1.0, 3.0))]"#;

    #[test]
    fn passthrough_values_round_trip() {
        use rr_core::hazard::HazardKind;
        use rr_core::map::{ItemKind, MoverKind};
        let m = built(PASSTHROUGH);
        assert_eq!(m.sectors[2].hazard.map(|h| h.kind), Some(HazardKind::Slime));
        let mover = m.sectors[1].mover.unwrap();
        assert_eq!(mover.kind, MoverKind::Lift { to: 1.5 });
        assert_eq!((mover.lock, mover.channel), (Some(Key::Red), Some(3)));
        assert!(m.sectors[1].secret && m.sectors[1].face_mat.is_some());
        assert!(!m.lights[0].shadows);
        assert_eq!(m.switches[0].key, Some(Key::Blue));
        assert_eq!(m.items[0].kind, ItemKind::Key(Key::Red));
        assert_eq!(m.actors[0].on_death, Some(SwitchAction::Exit));
        assert_eq!(m.walls.iter().filter(|w| w.glass).count(), 2);
        assert_eq!(m.music.as_deref(), Some("theme"));
        assert_eq!((m.quakes.len(), m.props.len()), (1, 1));
    }

    #[test]
    fn stairs_are_rejected_until_supported() {
        let e = build_source(&level(
            r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0))],
            stairs: [(id: "s", rect: (4.0, 0.0, 6.0, 4.0), dir: E, steps: 4, from_z: 0.0, to_z: 1.0)]"#,
        ))
        .unwrap_err();
        assert!(e.contains("stairs"), "{e}");
    }

    #[test]
    fn output_has_header_and_sector_id_comments() {
        let src = format!(
            "// My level\n// route here\n{}",
            level(r#"sectors: [(id: "lobby", rect: (0.0, 0.0, 4.0, 4.0))]"#)
        );
        let out = build_source(&src).unwrap();
        assert!(
            out.starts_with("// My level\n// route here\n// Built by `rr-tools build"),
            "{out}"
        );
        assert!(out.contains("// 0 lobby"), "{out}");
    }
}
