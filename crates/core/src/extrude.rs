//! Turns sectors into triangle meshes: floors, ceilings, solid walls and the upper/lower
//! "step" faces where a portal leads to a sector with a different floor or ceiling height.

use crate::map::{Map, MaterialId, SectorId};
use glam::{Vec2, Vec3};
use std::collections::BTreeMap;
use thiserror::Error;

/// World size, in metres, covered by one repeat of a texture.
pub const TEXTURE_WORLD_SIZE: f32 = 2.0;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn push_tri(&mut self, p: [Vec3; 3], normal: Vec3, uv: [Vec2; 3]) {
        let base = self.positions.len() as u32;
        for (pos, uv) in p.iter().zip(uv) {
            self.positions.push(pos.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    pub fn triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.indices
            .chunks_exact(3)
            .map(|t| [0, 1, 2].map(|k| Vec3::from_array(self.positions[t[k] as usize])))
    }

    pub fn area(&self) -> f32 {
        self.triangles()
            .map(|[a, b, c]| (b - a).cross(c - a).length() * 0.5)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubMesh {
    pub material: MaterialId,
    pub mesh: MeshData,
}

#[derive(Debug, Error, PartialEq)]
pub enum ExtrudeError {
    #[error("sector {0}: floor triangulation failed")]
    Triangulation(SectorId),
}

pub fn extrude_sector(map: &Map, s: SectorId) -> Result<Vec<SubMesh>, ExtrudeError> {
    let sector = &map.sectors[s];
    let mut out: BTreeMap<MaterialId, MeshData> = BTreeMap::new();
    let uv = |p: Vec2| p / TEXTURE_WORLD_SIZE;

    for [a, b, c] in triangulate(map, s)? {
        let (f, k) = (sector.floor_z, sector.ceil_z);
        out.entry(sector.floor_mat).or_default().push_tri(
            [a.extend(f), b.extend(f), c.extend(f)],
            Vec3::Z,
            [uv(a), uv(b), uv(c)],
        );
        // Reversed winding: the ceiling is seen from below.
        out.entry(sector.ceil_mat).or_default().push_tri(
            [a.extend(k), c.extend(k), b.extend(k)],
            Vec3::NEG_Z,
            [uv(a), uv(c), uv(b)],
        );
    }

    for wid in sector.walls() {
        let w = &map.walls[wid];
        match w.next_sector {
            None => wall_quad(
                out.entry(w.material).or_default(),
                w.a,
                w.b,
                sector.floor_z,
                sector.ceil_z,
            ),
            Some(t) => {
                let other = &map.sectors[t];
                let mesh = out.entry(other.face_mat.unwrap_or(w.material)).or_default();
                if other.floor_z > sector.floor_z {
                    wall_quad(
                        mesh,
                        w.a,
                        w.b,
                        sector.floor_z,
                        other.floor_z.min(sector.ceil_z),
                    );
                }
                if other.ceil_z < sector.ceil_z {
                    wall_quad(
                        mesh,
                        w.a,
                        w.b,
                        other.ceil_z.max(sector.floor_z),
                        sector.ceil_z,
                    );
                }
            }
        }
    }

    Ok(out
        .into_iter()
        .filter(|(_, m)| !m.indices.is_empty())
        .map(|(material, mesh)| SubMesh { material, mesh })
        .collect())
}

/// Vertical quad from z0 to z1 along a→b, facing left of a→b (into the owning sector).
fn wall_quad(mesh: &mut MeshData, a: Vec2, b: Vec2, z0: f32, z1: f32) {
    let d = b - a;
    let len = d.length();
    if z1 - z0 <= 1e-4 || len <= 1e-6 {
        return;
    }
    let normal = Vec2::new(-d.y, d.x).normalize().extend(0.0);
    let (a0, a1, b0, b1) = (a.extend(z0), a.extend(z1), b.extend(z0), b.extend(z1));
    // World-anchored V so textures line up across neighbouring sectors.
    let v = |z: f32| -z / TEXTURE_WORLD_SIZE;
    let u1 = len / TEXTURE_WORLD_SIZE;
    let (ua0, ua1) = (Vec2::new(0.0, v(z0)), Vec2::new(0.0, v(z1)));
    let (ub0, ub1) = (Vec2::new(u1, v(z0)), Vec2::new(u1, v(z1)));
    mesh.push_tri([a0, a1, b1], normal, [ua0, ua1, ub1]);
    mesh.push_tri([a0, b1, b0], normal, [ua0, ub1, ub0]);
}

/// Triangulates the sector polygon (outer loop + holes) into CCW triangles in the XY plane.
fn triangulate(map: &Map, s: SectorId) -> Result<Vec<[Vec2; 3]>, ExtrudeError> {
    let sector = &map.sectors[s];
    let mut pts: Vec<Vec2> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    for (i, lp) in sector.loops.iter().enumerate() {
        if i > 0 {
            holes.push(pts.len());
        }
        pts.extend(lp.iter().map(|&w| map.walls[w].a));
    }
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
    let idx = earcutr::earcut(&flat, &holes, 2).map_err(|_| ExtrudeError::Triangulation(s))?;
    Ok(idx
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            if (b - a).perp_dot(c - a) < 0.0 {
                [a, c, b]
            } else {
                [a, b, c]
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{pillar_room, two_rooms};

    fn by_material(subs: &[SubMesh], m: MaterialId) -> &MeshData {
        &subs
            .iter()
            .find(|s| s.material == m)
            .expect("material present")
            .mesh
    }

    fn assert_close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    #[test]
    fn room_with_lower_neighbour_gets_floor_ceiling_walls_and_step() {
        // B's floor is 1 m higher, so A shows a 1 m lower step on the shared wall.
        let map = two_rooms(1.0, 3.0);
        let subs = extrude_sector(&map, 0).unwrap();
        assert_close(by_material(&subs, 1).area(), 16.0); // floor
        assert_close(by_material(&subs, 2).area(), 16.0); // ceiling
        assert_close(by_material(&subs, 0).area(), 3.0 * 4.0 * 3.0 + 4.0 * 1.0); // 3 solid walls + step
    }

    #[test]
    fn higher_room_has_no_step_faces() {
        let map = two_rooms(1.0, 3.0);
        let subs = extrude_sector(&map, 1).unwrap();
        assert_close(by_material(&subs, 0).area(), 3.0 * 4.0 * 2.0); // 3 solid walls, 2 m tall
    }

    #[test]
    fn lower_neighbour_ceiling_makes_upper_step() {
        let map = two_rooms(0.0, 2.0);
        let subs = extrude_sector(&map, 0).unwrap();
        assert_close(by_material(&subs, 0).area(), 3.0 * 4.0 * 3.0 + 4.0 * 1.0);
    }

    #[test]
    fn pillar_hole_is_cut_from_floor_and_walled() {
        let map = pillar_room();
        let subs = extrude_sector(&map, 0).unwrap();
        assert_close(by_material(&subs, 1).area(), 100.0 - 4.0);
        assert_close(
            by_material(&subs, 0).area(),
            4.0 * 10.0 * 4.0 + 4.0 * 2.0 * 4.0,
        );
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_around_its_normal() {
        for map in [two_rooms(1.0, 2.0), pillar_room()] {
            for s in 0..map.sectors.len() {
                for sub in extrude_sector(&map, s).unwrap() {
                    let m = &sub.mesh;
                    for (t, tri) in m.triangles().enumerate() {
                        let geometric = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize();
                        let stored = Vec3::from_array(m.normals[m.indices[t * 3] as usize]);
                        assert!(
                            geometric.dot(stored) > 0.99,
                            "sector {s} tri {t}: {geometric} vs {stored}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn floor_faces_up_and_ceiling_faces_down() {
        let map = two_rooms(0.0, 3.0);
        let subs = extrude_sector(&map, 0).unwrap();
        assert!(
            by_material(&subs, 1)
                .normals
                .iter()
                .all(|n| *n == [0.0, 0.0, 1.0])
        );
        assert!(
            by_material(&subs, 2)
                .normals
                .iter()
                .all(|n| *n == [0.0, 0.0, -1.0])
        );
    }

    #[test]
    fn open_portal_with_equal_heights_emits_nothing_extra() {
        let map = two_rooms(0.0, 3.0);
        let subs = extrude_sector(&map, 0).unwrap();
        assert_close(by_material(&subs, 0).area(), 3.0 * 4.0 * 3.0);
    }

    #[test]
    fn closed_door_face_uses_the_door_sectors_face_material() {
        let mut map = crate::fixtures::door_rooms("(kind: Door)", "");
        assert!(
            extrude_sector(&map, 0)
                .unwrap()
                .iter()
                .all(|s| s.material != 3),
            "open: no face"
        );
        map.sectors[1].ceil_z = map.sectors[1].floor_z; // closed
        let subs = extrude_sector(&map, 0).unwrap();
        assert_close(by_material(&subs, 3).area(), 4.0 * 3.0);
    }
}
