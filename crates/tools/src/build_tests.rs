use super::*;
use rr_core::map::Map;

const DEFAULTS: &str =
    r#"defaults: (floor_z: 0.0, ceil_z: 3.0, floor: "concrete", ceil: "metal", wall: "brick")"#;

fn level(body: &str) -> String {
    format!(r#"(name: "t", {DEFAULTS}, player_start: (pos: (1.0, 1.0), angle_deg: 0.0), {body})"#)
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

#[test]
fn hole_sharing_an_outer_edge_is_an_error() {
    let e = build_source(&level(
        r#"sectors: [(id: "room", rect: (0.0, 0.0, 10.0, 10.0), holes: [[(4.0, 0.0), (6.0, 0.0), (6.0, 2.0), (4.0, 2.0)]])]"#,
    ))
    .unwrap_err();
    assert!(
        e.contains("\"room\"") && e.contains("hole shares an edge"),
        "{e}"
    );
}

#[test]
fn zero_width_spike_is_an_error() {
    let e = build_source(&level(
        r#"sectors: [(id: "spike", poly: [(0.0, 0.0), (4.0, 0.0), (8.0, 0.0), (4.0, 0.0002), (4.0, 4.0), (0.0, 4.0)])]"#,
    ))
    .unwrap_err();
    assert!(e.contains("\"spike\"") && e.contains("folds back"), "{e}");
}

#[test]
fn loop_through_a_vertex_twice_is_an_error() {
    // A bow-tie pinched at (4, 4): no edge runs both ways, but the loop crosses itself.
    let e = build_source(&level(
        r#"sectors: [(id: "bow", poly: [(0.0, 0.0), (4.0, 4.0), (8.0, 0.0), (8.0, 8.0), (4.0, 4.0), (0.0, 8.0)])]"#,
    ))
    .unwrap_err();
    assert!(e.contains("\"bow\"") && e.contains("twice"), "{e}");
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
fn stairs_expand_into_even_steps_climbing_dir() {
    let m = built(
        r#"sectors: [(id: "low", rect: (0.0, 0.0, 4.0, 4.0)), (id: "high", rect: (8.0, 0.0, 12.0, 4.0), floor_z: 2.0, ceil_z: 5.0)],
        stairs: [(id: "st", rect: (4.0, 0.0, 8.0, 4.0), dir: E, steps: 4, from_z: 0.5, to_z: 2.0, ceil_z: Some(5.0))]"#,
    );
    assert_eq!(m.sectors.len(), 6);
    let floors: Vec<f32> = m.sectors[2..].iter().map(|s| s.floor_z).collect();
    assert_eq!(floors, vec![0.5, 1.0, 1.5, 2.0]);
    assert!(m.sector_centre(2).x < m.sector_centre(5).x, "climbs east");
}

#[test]
fn stairs_with_too_tall_steps_error() {
    let e = build_source(&level(
        r#"sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0))],
        stairs: [(id: "st", rect: (4.0, 0.0, 8.0, 4.0), dir: E, steps: 2, from_z: 1.0, to_z: 3.0)]"#,
    ))
    .unwrap_err();
    assert!(e.contains("\"st\"") && e.contains("0.55"), "{e}");
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
