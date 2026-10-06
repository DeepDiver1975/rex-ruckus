//! Top-down SVG of a level for authoring and review. North is up; 1 m = 20 px.

use rr_core::glam::Vec2;
use rr_core::hazard::HazardKind;
use rr_core::map::{ActorKind, ItemKind, Key, Map, MoverKind, SwitchAction};
use rr_core::movement::Tuning;
use std::fmt::Write;

const SCALE: f32 = 20.0;
const MARGIN: f32 = 2.0;
/// Red, blue, yellow; indexed by `Key as usize`.
pub const KEY_COLORS: [&str; 3] = ["#e74c3c", "#3498db", "#f1c40f"];

pub const AMMO_COLOR: &str = "#f4d03f";
pub const SHELLS_COLOR: &str = "#e67e22";
pub const SHOTGUN_COLOR: &str = "#8b5a2b";
/// Glass panes: dashed cyan lines.
pub const GLASS_COLOR: &str = "#00e5ff";
/// Secret sectors: a star at the centre.
pub const SECRET_COLOR: &str = "#ffd700";

fn key_color(k: Key) -> &'static str {
    KEY_COLORS[k as usize]
}

/// Escapes text for use inside an XML element or attribute.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// SVG `points` of a five-pointed star centred on `c` (pixels), outer radius `r`.
fn star(c: Vec2, r: f32) -> String {
    (0..10)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let k = if i % 2 == 0 { r } else { r * 0.4 };
            format!("{:.1},{:.1}", c.x + k * a.cos(), c.y - k * a.sin())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extras for [`render_svg_with`].
#[derive(Default)]
pub struct SvgOptions<'a> {
    /// Label every vertex with its index; the slice is the level's raw vertex list.
    pub vertices: Option<&'a [(f32, f32)]>,
}

pub fn render_svg(map: &Map) -> String {
    render_svg_with(map, &SvgOptions::default())
}

/// The SVG path data of a sector's loops.
fn outline(map: &Map, loops: &[Vec<usize>], px: impl Fn(Vec2) -> Vec2) -> String {
    let mut d = String::new();
    for lp in loops {
        for (i, &w) in lp.iter().enumerate() {
            let p = px(map.walls[w].a);
            write!(
                d,
                "{}{:.1} {:.1} ",
                if i == 0 { "M" } else { "L" },
                p.x,
                p.y
            )
            .unwrap();
        }
        d.push_str("Z ");
    }
    d
}

pub fn render_svg_with(map: &Map, opts: &SvgOptions) -> String {
    let pts = map.walls.iter().flat_map(|w| [w.a, w.b]);
    let (min, max) = pts.fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    let px = |p: Vec2| {
        Vec2::new(
            (p.x - min.x + MARGIN) * SCALE,
            (max.y - p.y + MARGIN) * SCALE,
        )
    };
    let size = (max - min + Vec2::splat(2.0 * MARGIN)) * SCALE;
    let (lo, hi) = map.sectors.iter().fold((f32::MAX, f32::MIN), |(a, b), s| {
        (a.min(s.floor_z), b.max(s.floor_z))
    });
    let step = Tuning::default().step_height;

    let mut o = String::new();
    writeln!(o, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" font-family="monospace" font-size="10">"#, size.x, size.y).unwrap();
    writeln!(o, r##"<rect width="100%" height="100%" fill="#1d1f24"/><text x="6" y="14" fill="#ddd">{}</text>"##, esc(&map.name)).unwrap();
    // Crack walls are hatched: diagonal soot lines over brick red.
    // Slime is green with a dark diagonal; electric floors are blue with a yellow zigzag.
    o.push_str(r##"<defs><pattern id="hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="6" height="6" fill="#b5523b"/><line x1="0" y1="0" x2="0" y2="6" stroke="#2b1a14" stroke-width="3"/></pattern>"##);
    o.push_str(r##"<pattern id="slime" width="8" height="8" patternUnits="userSpaceOnUse"><rect width="8" height="8" fill="#3fae2a"/><line x1="0" y1="8" x2="8" y2="0" stroke="#1d4d14" stroke-width="2"/></pattern>"##);
    o.push_str(r##"<pattern id="electric" width="8" height="8" patternUnits="userSpaceOnUse"><rect width="8" height="8" fill="#2a6fd6"/><polyline points="0,6 2,2 4,6 6,2 8,6" style="fill:none" stroke="#ffe14d" stroke-width="1.5"/></pattern></defs>"##);
    o.push('\n');

    for (s, sec) in map.sectors.iter().enumerate() {
        let d = outline(map, &sec.loops, px);
        let (class, fill) = match sec.mover.map(|m| m.kind) {
            Some(MoverKind::Door) => ("sector door", "#e8a33d".to_string()),
            Some(MoverKind::Lift { .. }) => ("sector lift", "#5aa0e0".to_string()),
            Some(MoverKind::Crack) => ("sector crack", "url(#hatch)".to_string()),
            None if sec.hazard.is_some() => {
                let url = match sec.hazard.map(|h| h.kind) {
                    Some(HazardKind::Electric) => "url(#electric)",
                    _ => "url(#slime)",
                };
                ("sector hazard", url.to_string())
            }
            None => {
                let t = if hi > lo {
                    (sec.floor_z - lo) / (hi - lo)
                } else {
                    0.0
                };
                ("sector", format!("hsl(210, 10%, {:.0}%)", 30.0 + 40.0 * t))
            }
        };
        let stroke = sec.mover.and_then(|m| m.lock).map_or("none", key_color);
        writeln!(
            o,
            r#"<path class="{class}" d="{d}" fill="{fill}" fill-rule="evenodd" stroke="{stroke}" stroke-width="3"><title>sector {s}: floor {} ceil {}</title></path>"#,
            sec.floor_z, sec.ceil_z
        )
        .unwrap();
        let outer = &sec.loops[0];
        let c = px(outer.iter().map(|&w| map.walls[w].a).sum::<Vec2>() / outer.len() as f32);
        let label = match sec.mover.and_then(|m| m.channel) {
            Some(ch) => format!("{s} ch{ch}"),
            None => s.to_string(),
        };
        writeln!(
            o,
            r##"<text x="{:.1}" y="{:.1}" fill="#fff" text-anchor="middle">{label}</text>"##,
            c.x, c.y
        )
        .unwrap();
        if sec.secret {
            writeln!(
                o,
                r##"<polygon class="secret" points="{}" fill="{SECRET_COLOR}" stroke="#000"><title>secret</title></polygon>"##,
                star(c + Vec2::new(0.0, 12.0), 8.0)
            )
            .unwrap();
        }
    }

    for (id, w) in map.walls.iter().enumerate() {
        let (a, b) = (px(w.a), px(w.b));
        let style = match (w.next_sector, w.next_wall) {
            (None, _) => r##"stroke="#000" stroke-width="3""##.to_string(),
            (Some(_), Some(n)) if w.glass && id < n => {
                format!(
                    r#"class="glass" stroke="{GLASS_COLOR}" stroke-width="3" stroke-dasharray="6 3""#
                )
            }
            (Some(t), Some(n)) if id < n => {
                let ledge = (map.sectors[t].floor_z - map.sectors[w.sector].floor_z).abs() > step;
                format!(
                    r#"stroke="{}" stroke-width="1" stroke-dasharray="4 3""#,
                    if ledge { "#c0392b" } else { "#999" }
                )
            }
            _ => continue, // the mirrored half of a portal already drawn
        };
        writeln!(
            o,
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" {style}/>"#,
            a.x, a.y, b.x, b.y
        )
        .unwrap();
    }

    for sw in &map.switches {
        let w = &map.walls[sw.wall];
        let p = px((w.a + w.b) * 0.5 + w.inward_normal() * 0.3);
        let (fill, label) = match sw.action {
            SwitchAction::Exit => ("#ffd700", "EXIT".to_string()),
            SwitchAction::Channel(c) => ("#2ecc71", format!("ch{c}")),
        };
        let stroke = sw.key.map_or("#000", key_color);
        writeln!(
            o,
            r##"<rect class="switch" x="{:.1}" y="{:.1}" width="8" height="8" fill="{fill}" stroke="{stroke}" stroke-width="2"/><text x="{:.1}" y="{:.1}" fill="#fff">{label}</text>"##,
            p.x - 4.0, p.y - 4.0, p.x + 6.0, p.y + 4.0
        )
        .unwrap();
    }

    for item in &map.items {
        let p = px(item.pos);
        let title = format!("<title>{:?}</title>", item.kind);
        let (x, y) = (p.x, p.y);
        match item.kind {
            ItemKind::HealthSmall => writeln!(
                o,
                r##"<g class="item"><rect x="{:.1}" y="{:.1}" width="12" height="12" fill="#fff" stroke="#c0392b"/><path d="M{x:.1} {:.1}V{:.1}M{:.1} {y:.1}H{:.1}" stroke="#c0392b" stroke-width="3"/>{title}</g>"##,
                x - 6.0, y - 6.0, y - 4.0, y + 4.0, x - 4.0, x + 4.0
            ),
            kind => {
                let color = match kind {
                    ItemKind::Key(k) => key_color(k),
                    ItemKind::PistolAmmo => AMMO_COLOR,
                    ItemKind::ShotgunShells => SHELLS_COLOR,
                    _ => SHOTGUN_COLOR,
                };
                writeln!(
                    o,
                    r##"<circle class="item" cx="{x:.1}" cy="{y:.1}" r="6" fill="{color}" stroke="#fff">{title}</circle>"##
                )
            }
        }
        .unwrap();
    }

    for a in &map.actors {
        let (c, s) = (a.angle.cos(), a.angle.sin());
        // Triangle in metres: nose 0.5 ahead, base corners 0.35 behind and 0.35 to either side.
        let pt = |f: f32, l: f32| px(a.pos + Vec2::new(c * f - s * l, s * f + c * l));
        let (n, l, r) = (pt(0.5, 0.0), pt(-0.35, 0.35), pt(-0.35, -0.35));
        let (paint, state) = if a.asleep {
            (
                r##"fill="none" stroke="#e74c3c" stroke-width="2""##,
                "asleep",
            )
        } else {
            (
                r##"fill="#e74c3c" stroke="#fff" stroke-width="1""##,
                "awake",
            )
        };
        writeln!(
            o,
            r#"<polygon class="actor" points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" {paint}><title>{:?} ({state})</title></polygon>"#,
            n.x, n.y, l.x, l.y, r.x, r.y, a.kind
        )
        .unwrap();
    }

    for (i, l) in map.lights.iter().enumerate() {
        let p = px(Vec2::new(l.pos.0, l.pos.1));
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        writeln!(
            o,
            r##"<circle class="light" cx="{:.1}" cy="{:.1}" r="4" fill="rgb({},{},{})" stroke="#000"><title>light {i}</title></circle>"##,
            p.x, p.y, c(l.color.0), c(l.color.1), c(l.color.2)
        )
        .unwrap();
    }

    for p in &map.props {
        let pts = p
            .corners()
            .map(|c| {
                let q = px(c);
                format!("{:.1},{:.1}", q.x, q.y)
            })
            .join(" ");
        let (c, f) = (
            px(p.pos),
            px(p.pos + Vec2::from_angle(p.angle) * p.kind.half_extents().x * 1.5),
        );
        writeln!(
            o,
            r##"<polygon class="prop" points="{pts}" fill="#c8a2c8" stroke="#000"><title>{:?}</title></polygon><line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#000" stroke-width="2"/>"##,
            p.kind, c.x, c.y, f.x, f.y
        )
        .unwrap();
    }

    for t in &map.triggers {
        let sec = &map.sectors[t.sector];
        let outer = &sec.loops[0];
        let c = px(outer.iter().map(|&w| map.walls[w].a).sum::<Vec2>() / outer.len() as f32);
        let label = match t.action {
            SwitchAction::Exit => "EXIT".to_string(),
            SwitchAction::Channel(ch) => format!("ch{ch}"),
        };
        writeln!(
            o,
            r##"<path class="trigger" d="{}" fill="none" stroke="#ff00ff" stroke-dasharray="5 3" stroke-width="2"/><text x="{:.1}" y="{:.1}" fill="#ff00ff" text-anchor="middle">{label}</text>"##,
            outline(map, &sec.loops, px),
            c.x,
            c.y + 12.0
        )
        .unwrap();
    }

    for a in &map.actors {
        let p = px(a.pos);
        if a.kind == ActorKind::Boss {
            writeln!(
                o,
                r##"<circle class="boss" cx="{:.1}" cy="{:.1}" r="12" fill="none" stroke="#ff00ff" stroke-width="3"/>"##,
                p.x, p.y
            )
            .unwrap();
        }
        if let Some(action) = a.on_death {
            let what = match action {
                SwitchAction::Exit => "EXIT".to_string(),
                SwitchAction::Channel(c) => format!("ch{c}"),
            };
            writeln!(
                o,
                r##"<text x="{:.1}" y="{:.1}" fill="#ff00ff">on_death: {what}</text>"##,
                p.x + 14.0,
                p.y + 4.0
            )
            .unwrap();
        }
    }

    if let Some(verts) = opts.vertices {
        for (i, &(x, y)) in verts.iter().enumerate() {
            let p = px(Vec2::new(x, y));
            writeln!(
                o,
                r##"<text class="vid" x="{:.1}" y="{:.1}" fill="#8f8" font-size="8">{i}</text>"##,
                p.x + 2.0,
                p.y - 2.0
            )
            .unwrap();
        }
    }

    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    let a = map.player_start.angle_deg.to_radians();
    let (s, tip) = (px(start), px(start + Vec2::new(a.cos(), a.sin())));
    writeln!(
        o,
        r##"<circle class="start" cx="{:.1}" cy="{:.1}" r="5" fill="#2ecc71"/><line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#2ecc71" stroke-width="2"/>"##,
        s.x, s.y, s.x, s.y, tip.x, tip.y
    )
    .unwrap();
    o.push_str("</svg>\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::fixtures::{door_rooms, two_rooms};

    #[test]
    fn draws_every_sector_and_marker() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Red))",
            "switches: [(wall: (3, 4), action: Exit)], items: [(kind: Key(Red), pos: (2.0, 3.0))],",
        );
        let svg = render_svg(&map);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>\n"));
        assert_eq!(
            svg.matches("<path class=\"sector").count(),
            map.sectors.len()
        );
        assert_eq!(svg.matches("class=\"switch\"").count(), 1);
        assert_eq!(svg.matches("class=\"item\"").count(), 1);
        assert_eq!(svg.matches("class=\"start\"").count(), 1);
        assert!(svg.contains("class=\"sector door\""));
        assert!(svg.contains(KEY_COLORS[0]), "red lock/key colour present");
    }

    /// A hall (sector 0), a crack wall (sector 1) and a secret room (sector 2) behind it, and a
    /// glass booth (sector 3) on the hall's north side.
    fn destructibles() -> Map {
        Map::from_ron(
            r#"(
            name: "d", materials: ["m"],
            vertices: [(0.0, 0.0), (6.0, 0.0), (6.0, 2.0), (6.0, 4.0), (6.0, 6.0), (0.0, 6.0),
                       (6.5, 2.0), (6.5, 4.0), (6.5, 0.0), (10.0, 0.0), (10.0, 6.0), (6.5, 6.0),
                       (4.0, 6.0), (2.0, 6.0), (4.0, 9.0), (2.0, 9.0)],
            sectors: [
                (loops: [[0, 1, 2, 3, 4, 12, 13, 5]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
                (loops: [[2, 6, 7, 3]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0,
                 mover: Some((kind: Crack))),
                (loops: [[8, 9, 10, 11, 7, 6]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0,
                 secret: true),
                (loops: [[13, 12, 14, 15]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
            ],
            glass: [(12, 13)],
            player_start: (pos: (1.0, 1.0), angle_deg: 0.0),
        )"#,
        )
        .unwrap()
    }

    #[test]
    fn marks_glass_cracks_and_secrets() {
        let svg = render_svg(&destructibles());
        let glass: Vec<&str> = svg
            .lines()
            .filter(|l| l.contains("class=\"glass\""))
            .collect();
        assert_eq!(glass.len(), 1, "one line per pane: {svg}");
        assert!(
            glass[0].contains(GLASS_COLOR) && glass[0].contains("stroke-dasharray"),
            "{}",
            glass[0]
        );
        let crack = svg
            .lines()
            .find(|l| l.contains("class=\"sector crack\""))
            .expect("crack sector drawn");
        assert!(crack.contains("fill=\"url(#hatch)\""), "{crack}");
        assert!(svg.contains("<pattern id=\"hatch\""), "{svg}");
        assert_eq!(svg.matches("class=\"secret\"").count(), 1, "{svg}");
        // The plain map has none of them.
        let plain = render_svg(&two_rooms(0.0, 3.0));
        assert!(!plain.contains("class=\"glass\"") && !plain.contains("class=\"secret\""));
    }

    #[test]
    fn draws_engine_overlays() {
        let map = rr_core::fixtures::engine_room(
            "lights: [(pos: (3.0, 2.0, 3.5), color: (1.0, 1.0, 1.0), intensity: 500.0, range: 6.0)],
            actors: [(kind: Boss, pos: (3.0, 2.0), on_death: Some(Exit))],",
        );
        let svg = render_svg(&map);
        assert_eq!(svg.matches("class=\"sector hazard\"").count(), 1);
        assert_eq!(svg.matches("class=\"prop\"").count(), 3);
        assert_eq!(svg.matches("class=\"trigger\"").count(), 1);
        assert!(
            svg.contains(">ch7<")
                && svg.contains("class=\"boss\"")
                && svg.contains("on_death: EXIT")
        );
        assert_eq!(svg.matches("class=\"light\"").count(), 1);
        assert_eq!(svg.matches("class=\"vid\"").count(), 0);
        let verts = [(0.0, 0.0), (6.0, 0.0)];
        let ids = render_svg_with(
            &map,
            &SvgOptions {
                vertices: Some(&verts),
            },
        );
        assert_eq!(ids.matches("class=\"vid\"").count(), 2);
    }

    #[test]
    fn north_is_up() {
        // two_rooms: start (2,2) in a 4 m tall room; with a 2 m margin and 20 px/m, y=2 → (4+2-2)·20 = 80.
        let svg = render_svg(&two_rooms(0.0, 3.0));
        assert!(
            svg.contains("<circle class=\"start\" cx=\"80.0\" cy=\"80.0\""),
            "{svg}"
        );
    }

    #[test]
    fn level_name_is_xml_escaped() {
        let mut map = two_rooms(0.0, 3.0);
        map.name = "a<b&\"c\"".to_string();
        let svg = render_svg(&map);
        assert!(svg.contains("a&lt;b&amp;&quot;c&quot;"), "{svg}");
        assert!(!svg.contains("a<b"), "{svg}");
    }

    #[test]
    fn draws_actors_and_item_kinds() {
        let map = door_rooms(
            "(kind: Door)",
            "items: [(kind: Key(Red), pos: (1.0, 1.0)), (kind: PistolAmmo, pos: (1.0, 2.0)), (kind: ShotgunShells, pos: (1.0, 3.0)), (kind: Shotgun, pos: (2.0, 1.0)), (kind: HealthSmall, pos: (2.0, 3.0))], actors: [(kind: Grunt, pos: (6.0, 2.0), angle_deg: 90.0, asleep: true), (kind: Grunt, pos: (7.0, 2.0), asleep: false)],",
        );
        let svg = render_svg(&map);
        assert_eq!(svg.matches("class=\"actor\"").count(), 2, "{svg}");
        assert!(
            svg.contains("Grunt (asleep)") && svg.contains("Grunt (awake)"),
            "{svg}"
        );
        assert_eq!(svg.matches("fill=\"none\"").count(), 1, "one hollow actor");
        assert_eq!(svg.matches("class=\"item\"").count(), 5);
        for c in [AMMO_COLOR, SHELLS_COLOR, SHOTGUN_COLOR, KEY_COLORS[0]] {
            assert!(svg.contains(c), "{c}");
        }
        for t in [
            "PistolAmmo",
            "ShotgunShells",
            "Shotgun",
            "HealthSmall",
            "Key(Red)",
        ] {
            assert!(svg.contains(&format!("<title>{t}</title>")), "{t}");
        }
        assert!(!svg.contains("#888"), "grey fallback is gone");

        // Heading: the first polygon vertex is the nose. 90 deg = north = up (smaller SVG y).
        let pts = |needle: &str| -> Vec<(f32, f32)> {
            let tag = svg
                .lines()
                .find(|l| l.contains("class=\"actor\"") && l.contains(needle))
                .unwrap();
            let p = tag
                .split("points=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            p.split(' ')
                .map(|v| {
                    let (x, y) = v.split_once(',').unwrap();
                    (x.parse().unwrap(), y.parse().unwrap())
                })
                .collect()
        };
        let north = pts("fill=\"none\"");
        assert!(
            north[0].1 < north[1].1 && north[0].1 < north[2].1,
            "{north:?}"
        );
        assert!(
            (north[0].0 - (north[1].0 + north[2].0) / 2.0).abs() < 0.2,
            "{north:?}"
        );
        let east = pts("fill=\"#e74c3c\"");
        assert!(east[0].0 > east[1].0 && east[0].0 > east[2].0, "{east:?}");
        assert!(
            (east[0].1 - (east[1].1 + east[2].1) / 2.0).abs() < 0.2,
            "{east:?}"
        );
    }
}
