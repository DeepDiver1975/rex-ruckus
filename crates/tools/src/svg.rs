//! Top-down SVG of a level for authoring and review. North is up; 1 m = 20 px.

use rr_core::glam::Vec2;
use rr_core::map::{ItemKind, Key, Map, MoverKind, SwitchAction};
use rr_core::movement::Tuning;
use std::fmt::Write;

const SCALE: f32 = 20.0;
const MARGIN: f32 = 2.0;
/// Red, blue, yellow; indexed by `Key as usize`.
pub const KEY_COLORS: [&str; 3] = ["#e74c3c", "#3498db", "#f1c40f"];

pub const AMMO_COLOR: &str = "#f4d03f";
pub const SHELLS_COLOR: &str = "#e67e22";
pub const SHOTGUN_COLOR: &str = "#8b5a2b";

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

pub fn render_svg(map: &Map) -> String {
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

    for (s, sec) in map.sectors.iter().enumerate() {
        let mut d = String::new();
        for lp in &sec.loops {
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
        let (class, fill) = match sec.mover.map(|m| m.kind) {
            Some(MoverKind::Door) => ("sector door", "#e8a33d".to_string()),
            Some(MoverKind::Lift { .. }) => ("sector lift", "#5aa0e0".to_string()),
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
    }

    for (id, w) in map.walls.iter().enumerate() {
        let (a, b) = (px(w.a), px(w.b));
        let style = match (w.next_sector, w.next_wall) {
            (None, _) => r##"stroke="#000" stroke-width="3""##.to_string(),
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
    }
}
