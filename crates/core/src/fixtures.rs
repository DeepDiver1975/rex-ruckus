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
