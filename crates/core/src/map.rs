//! Level format (RON) and the built, linked map.
//!
//! Authoring rule: each sector lists vertex-index loops. The first loop is the outer boundary,
//! counter-clockwise; further loops are holes, clockwise. A wall a→b becomes a portal into the
//! sector that owns the edge b→a.

use glam::Vec2;
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

pub type SectorId = usize;
pub type WallId = usize;
pub type MaterialId = usize;

#[derive(Debug, Clone, Deserialize)]
pub struct RawLevel {
    pub name: String,
    pub materials: Vec<String>,
    pub vertices: Vec<(f32, f32)>,
    pub sectors: Vec<RawSector>,
    pub player_start: PlayerStart,
    #[serde(default)]
    pub lights: Vec<RawLight>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawSector {
    pub loops: Vec<Vec<usize>>,
    pub floor_z: f32,
    pub ceil_z: f32,
    pub floor_mat: MaterialId,
    pub ceil_mat: MaterialId,
    pub wall_mat: MaterialId,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PlayerStart {
    pub pos: (f32, f32),
    pub angle_deg: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawLight {
    pub pos: (f32, f32, f32),
    pub color: (f32, f32, f32),
    /// Luminous power in lumens.
    pub intensity: f32,
    pub range: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    pub a: Vec2,
    pub b: Vec2,
    pub sector: SectorId,
    pub next_sector: Option<SectorId>,
    pub next_wall: Option<WallId>,
    pub material: MaterialId,
}

impl Wall {
    /// Unit normal pointing into this wall's own sector (left of a→b).
    pub fn inward_normal(&self) -> Vec2 {
        let d = (self.b - self.a).normalize_or_zero();
        Vec2::new(-d.y, d.x)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sector {
    pub loops: Vec<Vec<WallId>>,
    pub floor_z: f32,
    pub ceil_z: f32,
    pub floor_mat: MaterialId,
    pub ceil_mat: MaterialId,
    pub wall_mat: MaterialId,
}

impl Sector {
    pub fn walls(&self) -> impl Iterator<Item = WallId> + '_ {
        self.loops.iter().flatten().copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    pub name: String,
    pub materials: Vec<String>,
    pub sectors: Vec<Sector>,
    pub walls: Vec<Wall>,
    pub player_start: PlayerStart,
    pub lights: Vec<RawLight>,
}

#[derive(Debug, Error, PartialEq)]
pub enum MapError {
    #[error("RON parse error: {0}")]
    Parse(String),
    #[error("sector {sector}: vertex index {index} out of range")]
    BadVertex { sector: SectorId, index: usize },
    #[error("sector {sector}: loop {loop_index} has fewer than 3 vertices")]
    ShortLoop { sector: SectorId, loop_index: usize },
    #[error("sector {sector}: floor_z must be below ceil_z")]
    InvertedHeights { sector: SectorId },
    #[error("sector {sector}: material {material} out of range")]
    BadMaterial {
        sector: SectorId,
        material: MaterialId,
    },
    #[error("edge {from}->{to} is used by more than one sector in the same direction")]
    DuplicateEdge { from: usize, to: usize },
}

impl Map {
    pub fn from_ron(src: &str) -> Result<Map, MapError> {
        let raw: RawLevel = ron::from_str(src).map_err(|e| MapError::Parse(e.to_string()))?;
        Map::from_raw(raw)
    }

    pub fn from_raw(raw: RawLevel) -> Result<Map, MapError> {
        let verts: Vec<Vec2> = raw.vertices.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let mut walls: Vec<Wall> = Vec::new();
        let mut sectors = Vec::with_capacity(raw.sectors.len());
        let mut edge_owner: HashMap<(usize, usize), WallId> = HashMap::new();

        for (si, rs) in raw.sectors.iter().enumerate() {
            if rs.floor_z >= rs.ceil_z {
                return Err(MapError::InvertedHeights { sector: si });
            }
            for material in [rs.floor_mat, rs.ceil_mat, rs.wall_mat] {
                if material >= raw.materials.len() {
                    return Err(MapError::BadMaterial {
                        sector: si,
                        material,
                    });
                }
            }
            let mut loops = Vec::with_capacity(rs.loops.len());
            for (li, lp) in rs.loops.iter().enumerate() {
                if lp.len() < 3 {
                    return Err(MapError::ShortLoop {
                        sector: si,
                        loop_index: li,
                    });
                }
                let mut ids = Vec::with_capacity(lp.len());
                for i in 0..lp.len() {
                    let (ia, ib) = (lp[i], lp[(i + 1) % lp.len()]);
                    for index in [ia, ib] {
                        if index >= verts.len() {
                            return Err(MapError::BadVertex { sector: si, index });
                        }
                    }
                    if edge_owner.insert((ia, ib), walls.len()).is_some() {
                        return Err(MapError::DuplicateEdge { from: ia, to: ib });
                    }
                    ids.push(walls.len());
                    walls.push(Wall {
                        a: verts[ia],
                        b: verts[ib],
                        sector: si,
                        next_sector: None,
                        next_wall: None,
                        material: rs.wall_mat,
                    });
                }
                loops.push(ids);
            }
            sectors.push(Sector {
                loops,
                floor_z: rs.floor_z,
                ceil_z: rs.ceil_z,
                floor_mat: rs.floor_mat,
                ceil_mat: rs.ceil_mat,
                wall_mat: rs.wall_mat,
            });
        }

        for (&(ia, ib), &wid) in &edge_owner {
            if let Some(&other) = edge_owner.get(&(ib, ia)) {
                walls[wid].next_sector = Some(walls[other].sector);
                walls[wid].next_wall = Some(other);
            }
        }

        Ok(Map {
            name: raw.name,
            materials: raw.materials,
            sectors,
            walls,
            player_start: raw.player_start,
            lights: raw.lights,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{pillar_room, two_rooms};

    #[test]
    fn shared_edge_becomes_a_mirrored_portal() {
        let map = two_rooms(0.0, 3.0);
        let portals: Vec<&Wall> = map
            .walls
            .iter()
            .filter(|w| w.next_sector.is_some())
            .collect();
        assert_eq!(portals.len(), 2);
        let a_side = portals.iter().find(|w| w.sector == 0).unwrap();
        assert_eq!(a_side.next_sector, Some(1));
        assert_eq!(a_side.a, Vec2::new(4.0, 0.0));
        assert_eq!(a_side.b, Vec2::new(4.0, 4.0));
        let back = &map.walls[a_side.next_wall.unwrap()];
        assert_eq!((back.sector, back.next_sector), (1, Some(0)));
        assert_eq!((back.a, back.b), (a_side.b, a_side.a));
    }

    #[test]
    fn hole_loops_are_kept_and_solid() {
        let map = pillar_room();
        assert_eq!(map.sectors[0].loops.len(), 2);
        assert_eq!(map.walls.len(), 8);
        assert!(map.walls.iter().all(|w| w.next_sector.is_none()));
    }

    #[test]
    fn inward_normal_points_into_ccw_sector() {
        let map = two_rooms(0.0, 3.0);
        // First wall of A runs (0,0)->(4,0); the room interior is +y.
        assert_eq!(map.walls[0].inward_normal(), Vec2::new(0.0, 1.0));
    }

    fn level(vertices: &str, sectors: &str) -> String {
        format!(
            r#"(name: "t", materials: ["m"], vertices: [{vertices}], sectors: [{sectors}],
                player_start: (pos: (0.5, 0.5), angle_deg: 0.0))"#
        )
    }

    #[test]
    fn rejects_out_of_range_vertex() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,7]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadVertex {
                sector: 0,
                index: 7
            }
        );
    }

    #[test]
    fn rejects_short_loop() {
        let src = level(
            "(0.0,0.0),(1.0,0.0)",
            "(loops: [[0,1]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::ShortLoop {
                sector: 0,
                loop_index: 0
            }
        );
    }

    #[test]
    fn rejects_inverted_heights() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 2.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::InvertedHeights { sector: 0 }
        );
    }

    #[test]
    fn rejects_bad_material() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 3, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadMaterial {
                sector: 0,
                material: 3
            }
        );
    }

    #[test]
    fn rejects_duplicate_directed_edge() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
             (loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::DuplicateEdge { from: 0, to: 1 }
        );
    }

    #[test]
    fn reports_parse_errors() {
        assert!(matches!(
            Map::from_ron("(nonsense"),
            Err(MapError::Parse(_))
        ));
    }
}
