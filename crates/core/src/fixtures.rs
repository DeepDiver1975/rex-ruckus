//! Small hand-made maps shared by unit tests, integration tests and the game crate's tests.

use crate::map::Map;

/// Two 4×4 m rooms side by side. A = x∈[0,4] (floor 0, ceiling 3); B = x∈[4,8] with the given
/// heights. Materials: 0 = wall, 1 = floor, 2 = ceiling. Start (2,2) facing east.
pub fn two_rooms(b_floor: f32, b_ceil: f32) -> Map {
    let src = format!(
        r#"(
        name: "two rooms",
        materials: ["wall", "floor", "ceiling"],
        vertices: [(0.0, 0.0), (4.0, 0.0), (8.0, 0.0), (8.0, 4.0), (4.0, 4.0), (0.0, 4.0)],
        sectors: [
            (loops: [[0, 1, 4, 5]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[1, 2, 3, 4]], floor_z: {b_floor:?}, ceil_z: {b_ceil:?}, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
        ],
        player_start: (pos: (2.0, 2.0), angle_deg: 0.0),
    )"#
    );
    Map::from_ron(&src).expect("two_rooms fixture is valid")
}

/// A 10×10 m room (floor 0, ceiling 4) with a 2×2 m pillar hole at x,y∈[4,6].
/// Start (2,1) facing north.
pub fn pillar_room() -> Map {
    Map::from_ron(
        r#"(
        name: "pillar room",
        materials: ["wall", "floor", "ceiling"],
        vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0),
                   (4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0)],
        sectors: [
            (loops: [[0, 1, 2, 3], [4, 5, 6, 7]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
        ],
        player_start: (pos: (2.0, 1.0), angle_deg: 90.0),
    )"#,
    )
    .expect("pillar_room fixture is valid")
}

/// Three cells in a row along x, all y∈[0,4] with ceiling `ceil`: A x∈[0,4] (floor 0),
/// M x∈[4,4+mid_w] (floor 0, plus the RON fields in `mid`), and B, 4 m wide, with floor `b_floor`.
/// Materials: 0 wall, 1 floor, 2 ceiling, 3 door. Start (2,2) facing east. `extra` is spliced
/// into the level (e.g. `switches: [...],`). Vertex 7→0 is A's west wall; 3→4 is B's east wall.
fn three_cells(mid_w: f32, b_floor: f32, ceil: f32, mid: &str, extra: &str) -> Map {
    let (x1, x2) = (4.0 + mid_w, 8.0 + mid_w);
    let src = format!(
        r#"(
        name: "three cells",
        materials: ["wall", "floor", "ceiling", "door"],
        vertices: [(0.0, 0.0), (4.0, 0.0), ({x1:?}, 0.0), ({x2:?}, 0.0), ({x2:?}, 4.0), ({x1:?}, 4.0), (4.0, 4.0), (0.0, 4.0)],
        sectors: [
            (loops: [[0, 1, 6, 7]], floor_z: 0.0, ceil_z: {ceil:?}, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[1, 2, 5, 6]], floor_z: 0.0, ceil_z: {ceil:?}, floor_mat: 1, ceil_mat: 2, wall_mat: 0, {mid}),
            (loops: [[2, 3, 4, 5]], floor_z: {b_floor:?}, ceil_z: {ceil:?}, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
        ],
        player_start: (pos: (2.0, 2.0), angle_deg: 0.0),
        {extra}
    )"#
    );
    Map::from_ron(&src).expect("three_cells fixture is valid")
}

/// Room A x∈[0,4], a 0.5 m door sector (open height 3, `face_mat` 3) at x∈[4,4.5], room B x∈[4.5,8.5].
/// `door` is the mover RON, e.g. `"(kind: Door)"`.
pub fn door_rooms(door: &str, extra: &str) -> Map {
    three_cells(
        0.5,
        0.0,
        3.0,
        &format!("face_mat: Some(3), mover: Some({door})"),
        extra,
    )
}

/// Room A (floor 0) x∈[0,4], a 2 m lift at x∈[4,6] starting at floor 0, ledge B (floor 2) x∈[6,10];
/// ceilings 6. `lift` is the mover RON, e.g. `"(kind: Lift(to: 2.0))"`.
pub fn lift_shaft(lift: &str, extra: &str) -> Map {
    three_cells(2.0, 2.0, 6.0, &format!("mover: Some({lift})"), extra)
}

/// A thin triangle with a sharp corner at the origin: (0,0), (10,0), (10,h). Floor 0, ceiling 3.
/// Start (8, 0.4·h).
pub fn wedge(h: f32) -> Map {
    let y = 0.4 * h;
    Map::from_ron(&format!(
        r#"(name: "wedge", materials: ["wall", "floor", "ceiling"],
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, {h:?})],
            sectors: [(loops: [[0, 1, 2]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0)],
            player_start: (pos: (8.0, {y:?}), angle_deg: 180.0))"#
    ))
    .expect("wedge fixture is valid")
}

/// A 10×10 m hall A (sector 0, floor 0, ceiling 4) with a 2×2 m pillar hole at x,y∈[4,6],
/// and a 4×2 m booth B (sector 1, x∈[10,14], y∈[4,6], floor 0, ceiling 3) behind a glass pane
/// on the shared wall x = 10. Materials: 0 wall, 1 floor, 2 ceiling, 3 glass (added at load).
/// Start (8,5) facing east, at the pane.
pub fn glass_rooms() -> Map {
    Map::from_ron(
        r#"(
        name: "glass rooms",
        materials: ["wall", "floor", "ceiling"],
        vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (10.0, 6.0), (10.0, 10.0), (0.0, 10.0),
                   (4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0), (14.0, 4.0), (14.0, 6.0)],
        sectors: [
            (loops: [[0, 1, 2, 3, 4, 5], [6, 7, 8, 9]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[2, 10, 11, 3]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
        ],
        player_start: (pos: (8.0, 5.0), angle_deg: 0.0),
        glass: [(2, 3)],
    )"#,
    )
    .expect("glass_rooms fixture is valid")
}

/// The shipped weapon and enemy defs.
pub fn defs() -> crate::defs::Defs {
    crate::defs::Defs::builtin()
}

/// The M3 shooting range: a 12×8 m main room (sector 0, floor 0, ceiling 4) with a 2×2 m pillar
/// hole at x,y∈[3,5]; a 0.5 m step (sector 1, x∈[8,12], y∈[0,3], floor 0.5); a soffit (sector 2,
/// x∈[8,12], y∈[5,8], ceiling 2.5); a 0.5 m door (sector 3, x∈[12,12.5], y∈[3,5], open height 3,
/// `face_mat` 3) leading to room B (sector 4, x∈[12.5,16.5], y∈[0,8], floor 0, ceiling 4).
/// Materials: 0 wall, 1 floor, 2 ceiling, 3 door. Start (1.5,1.5) facing east. Doors are open as
/// authored; `Mechanics::new` closes them.
pub fn combat_room() -> Map {
    Map::from_ron(
        r#"(
        name: "combat room",
        materials: ["wall", "floor", "ceiling", "door"],
        vertices: [
            (0.0, 0.0), (8.0, 0.0), (8.0, 3.0), (12.0, 3.0), (12.0, 5.0), (8.0, 5.0), (8.0, 8.0), (0.0, 8.0),
            (3.0, 3.0), (3.0, 5.0), (5.0, 5.0), (5.0, 3.0),
            (12.0, 0.0), (12.0, 8.0),
            (12.5, 3.0), (12.5, 5.0),
            (12.5, 0.0), (16.5, 0.0), (16.5, 8.0), (12.5, 8.0),
        ],
        sectors: [
            (loops: [[0, 1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[1, 12, 3, 2]], floor_z: 0.5, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[5, 4, 13, 6]], floor_z: 0.0, ceil_z: 2.5, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
            (loops: [[3, 14, 15, 4]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0,
             face_mat: Some(3), mover: Some((kind: Door))),
            (loops: [[16, 17, 18, 19, 15, 14]], floor_z: 0.0, ceil_z: 4.0, floor_mat: 1, ceil_mat: 2, wall_mat: 0),
        ],
        player_start: (pos: (1.5, 1.5), angle_deg: 0.0),
    )"#,
    )
    .expect("combat_room fixture is valid")
}
