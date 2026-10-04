use rr_core::collide::Body;
use rr_core::extrude::extrude_sector;
use rr_core::glam::Vec2;
use rr_core::map::Map;
use rr_core::movement::{MoveInput, Tuning, step_player};

const SRC: &str = include_str!("../../../assets/levels/test_yard.ron");
const DT: f32 = 1.0 / 60.0;

fn yard() -> Map {
    Map::from_ron(SRC).expect("test_yard.ron parses")
}

/// Walks straight towards `target` for up to `max_ticks`; returns the final body.
fn walk_to(map: &Map, mut b: Body, target: Vec2, max_ticks: usize) -> Body {
    let t = Tuning::default();
    for _ in 0..max_ticks {
        let to = target - b.pos.truncate();
        if to.length() < 0.2 {
            break;
        }
        let input = MoveInput {
            wish: to.normalize(),
            ..Default::default()
        };
        step_player(map, &mut b, &input, &t, DT);
    }
    b
}

#[test]
fn level_links_expected_portals() {
    let map = yard();
    assert_eq!(map.sectors.len(), 8);
    let mut neighbours: Vec<usize> = map.sectors[0]
        .walls()
        .filter_map(|w| map.walls[w].next_sector)
        .collect();
    neighbours.sort();
    assert_eq!(neighbours, vec![1, 5, 7]);
}

#[test]
fn every_sector_extrudes() {
    let map = yard();
    for s in 0..map.sectors.len() {
        assert!(!extrude_sector(&map, s).unwrap().is_empty(), "sector {s}");
    }
}

#[test]
fn player_can_climb_stairs_to_platform() {
    let map = yard();
    let start = Vec2::new(map.player_start.pos.0, map.player_start.pos.1);
    let b = Body::spawn(&map, start, 0.35, 1.8).expect("start inside a sector");
    assert_eq!(b.sector, 0);
    let b = walk_to(&map, b, Vec2::new(14.0, 8.0), 300);
    let b = walk_to(&map, b, Vec2::new(22.0, 8.0), 300);
    assert_eq!(b.sector, 4);
    assert_eq!(b.pos.z, 1.2);
}

#[test]
fn player_cannot_step_onto_high_ledge() {
    let map = yard();
    let b = Body::spawn(&map, Vec2::new(3.0, 8.0), 0.35, 1.8).unwrap();
    let b = walk_to(&map, b, Vec2::new(-2.0, 8.0), 120);
    assert_eq!(b.sector, 0);
    assert!(b.pos.x >= 0.35 - 1e-3);
}

#[test]
fn player_walks_through_doorway_into_room() {
    let map = yard();
    let b = Body::spawn(&map, Vec2::new(8.0, 12.0), 0.35, 1.8).unwrap();
    let b = walk_to(&map, b, Vec2::new(8.0, 21.0), 300);
    assert_eq!(b.sector, 6);
}
