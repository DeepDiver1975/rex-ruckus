//! `rr-tools info`: a text summary of a level for authors (sectors, channel wiring, counts).

use rr_core::difficulty::Difficulty;
use rr_core::hazard::HazardKind;
use rr_core::map::{Channel, Map, MoverKind, RawLevel, SwitchAction};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Absolute shoelace area of a loop of raw vertex ids.
fn loop_area(raw: &RawLevel, lp: &[usize]) -> f32 {
    let v = |i: usize| raw.vertices[i];
    let sum: f32 = (0..lp.len())
        .map(|k| {
            let (a, b) = (v(lp[k]), v(lp[(k + 1) % lp.len()]));
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    sum.abs() * 0.5
}

#[derive(Default)]
struct Wiring {
    sources: Vec<String>,
    listeners: Vec<String>,
}

pub fn level_info(raw: &RawLevel, map: &Map) -> String {
    let mut o = String::new();
    let (lo, hi) = raw.vertices.iter().fold(
        ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)),
        |(lo, hi), &(x, y)| ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y))),
    );
    writeln!(
        o,
        "{}: {} sectors, {} walls, {} vertices, bbox ({:.1}, {:.1})..({:.1}, {:.1})",
        raw.name,
        map.sectors.len(),
        map.walls.len(),
        raw.vertices.len(),
        lo.0,
        lo.1,
        hi.0,
        hi.1
    )
    .unwrap();

    writeln!(o, "sector  floor   ceil    area  vertices  flags").unwrap();
    for (s, (rs, sec)) in raw.sectors.iter().zip(&map.sectors).enumerate() {
        let verts = rs
            .loops
            .iter()
            .map(|lp| {
                lp.iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let area = rs.loops.first().map_or(0.0, |l| loop_area(raw, l))
            - rs.loops
                .iter()
                .skip(1)
                .map(|l| loop_area(raw, l))
                .sum::<f32>();
        let mut flags: Vec<String> = Vec::new();
        if let Some(m) = sec.mover {
            flags.push(match m.kind {
                MoverKind::Door => "door".into(),
                MoverKind::Lift { to } => format!("lift->{to:.2}"),
                MoverKind::Crack => "crack".into(),
            });
            if let Some(k) = m.lock {
                flags.push(format!("lock:{}", k.name()));
            }
            if let Some(c) = m.channel {
                flags.push(format!("ch{c}"));
            }
            if m.one_shot {
                flags.push("one-shot".into());
            }
            if let Some(t) = m.auto_return {
                flags.push(format!("auto {t}s"));
            }
        }
        if let Some(h) = sec.hazard {
            let kind = match h.kind {
                HazardKind::Slime => "slime",
                HazardKind::Electric => "electric",
            };
            flags.push(format!("{kind} {}/{}s", h.damage, h.interval));
        }
        if sec.secret {
            flags.push("secret".into());
        }
        let row = format!(
            "{s:>6} {:>6.2} {:>6.2} {area:>7.1}  {verts}  {}",
            sec.floor_z,
            sec.ceil_z,
            flags.join(" ")
        );
        writeln!(o, "{}", row.trim_end()).unwrap();
    }

    let mut channels: BTreeMap<Channel, Wiring> = BTreeMap::new();
    let mut exits: Vec<String> = Vec::new();
    let mut fire = |action: SwitchAction, who: String| match action {
        SwitchAction::Channel(c) => channels.entry(c).or_default().sources.push(who),
        SwitchAction::Exit => exits.push(who),
    };
    for (i, sw) in map.switches.iter().enumerate() {
        fire(sw.action, format!("switch {i}"));
    }
    for (i, t) in map.triggers.iter().enumerate() {
        fire(t.action, format!("trigger {i} (sector {})", t.sector));
    }
    for (i, a) in map.actors.iter().enumerate() {
        if let Some(action) = a.on_death {
            fire(action, format!("on_death actor {i} ({:?})", a.kind));
        }
    }
    for (s, sec) in map.sectors.iter().enumerate() {
        if let Some(c) = sec.mover.and_then(|m| m.channel) {
            channels
                .entry(c)
                .or_default()
                .listeners
                .push(format!("mover in sector {s}"));
        }
    }
    for (i, q) in map.quakes.iter().enumerate() {
        channels
            .entry(q.channel)
            .or_default()
            .listeners
            .push(format!("quake {i}"));
    }
    let list = |v: &[String]| {
        if v.is_empty() {
            "nothing".to_string()
        } else {
            v.join(", ")
        }
    };
    writeln!(o, "channels:").unwrap();
    if channels.is_empty() {
        writeln!(o, "  none").unwrap();
    }
    for (c, w) in &channels {
        writeln!(o, "  ch{c}: {} -> {}", list(&w.sources), list(&w.listeners)).unwrap();
    }
    writeln!(
        o,
        "exits: {}",
        if exits.is_empty() {
            "none".to_string()
        } else {
            exits.join(", ")
        }
    )
    .unwrap();

    let counts = |count: &dyn Fn(&Map) -> usize| {
        Difficulty::ALL
            .iter()
            .map(|&d| format!("{} {}", d.label(), count(&map.for_difficulty(d))))
            .collect::<Vec<_>>()
            .join("  ")
    };
    writeln!(o, "actors  {}", counts(&|m| m.actors.len())).unwrap();
    writeln!(o, "items   {}", counts(&|m| m.items.len())).unwrap();
    let props = map
        .props
        .iter()
        .map(|p| match p.kind {
            rr_core::props::PropKind::Vending => format!("{:?} (stock {})", p.kind, p.stock),
            k => format!("{k:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        o,
        "props   {}{}{props}",
        map.props.len(),
        if props.is_empty() { "" } else { ": " }
    )
    .unwrap();
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::fixtures::engine_room_ron;

    #[test]
    fn info_lists_sectors_channels_and_counts() {
        let src = engine_room_ron("actors: [(kind: Grunt, pos: (3.0, 2.0), skill: Hard)],");
        let raw: RawLevel = ron::from_str(&src).unwrap();
        let map = Map::from_raw(raw.clone()).unwrap();
        let s = level_info(&raw, &map);
        assert!(
            s.starts_with(
                "engine room: 5 sectors, 20 walls, 12 vertices, bbox (0.0, 0.0)..(19.0, 4.0)"
            ),
            "{s}"
        );
        assert!(
            s.contains("     2   0.00   4.00     8.0  2 3 8 9  lift->-2.00 ch7 one-shot"),
            "{s}"
        );
        assert!(s.contains("slime 4/0.75s"), "{s}");
        assert!(
            s.contains("ch7: trigger 0 (sector 1) -> mover in sector 2, quake 0"),
            "{s}"
        );
        assert!(s.contains("exits: none"), "{s}");
        assert!(s.contains("actors  Easy 0  Normal 0  Hard 1"), "{s}");
        assert!(
            s.contains("props   3: Toilet, Vending (stock 2), PoolTable"),
            "{s}"
        );
    }
}
