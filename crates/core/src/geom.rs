//! Point/segment helpers and sector lookup.

use crate::map::{Map, SectorId};
use glam::Vec2;

pub fn closest_point_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

impl Map {
    /// Sectors on the far side of this sector's portals (one entry per portal wall).
    pub fn neighbours(&self, s: SectorId) -> impl Iterator<Item = SectorId> + '_ {
        self.sectors[s]
            .walls()
            .filter_map(|w| self.walls[w].next_sector)
    }

    /// Sectors a body, a shot or a sound can pass into from this sector: one entry per portal
    /// wall that holds no intact glass.
    pub fn passages(&self, s: SectorId) -> impl Iterator<Item = SectorId> + '_ {
        self.sectors[s]
            .walls()
            .filter_map(|w| self.walls[w].passage())
    }

    /// Even-odd test over all of the sector's loops, so holes are excluded automatically.
    /// Points exactly on the bottom/left edge count as inside (half-open rule), which makes a
    /// shared portal line belong to exactly one of its two sectors.
    pub fn sector_contains(&self, s: SectorId, p: Vec2) -> bool {
        let mut inside = false;
        for wid in self.sectors[s].walls() {
            let (a, b) = (self.walls[wid].a, self.walls[wid].b);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Like Build's `updatesector`: try the hint, then its portal neighbours, then everything.
    pub fn find_sector(&self, p: Vec2, hint: Option<SectorId>) -> Option<SectorId> {
        if let Some(h) = hint {
            if self.sector_contains(h, p) {
                return Some(h);
            }
            for wid in self.sectors[h].walls() {
                if let Some(n) = self.walls[wid].next_sector
                    && self.sector_contains(n, p)
                {
                    return Some(n);
                }
            }
        }
        (0..self.sectors.len()).find(|&s| self.sector_contains(s, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{pillar_room, two_rooms};

    #[test]
    fn closest_point_clamps_to_segment_ends() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0));
        assert_eq!(
            closest_point_on_segment(Vec2::new(2.0, 3.0), a, b),
            Vec2::new(2.0, 0.0)
        );
        assert_eq!(closest_point_on_segment(Vec2::new(-5.0, 1.0), a, b), a);
        assert_eq!(closest_point_on_segment(Vec2::new(9.0, -1.0), a, b), b);
        assert_eq!(closest_point_on_segment(Vec2::new(1.0, 1.0), a, a), a);
    }

    #[test]
    fn contains_respects_holes() {
        let map = pillar_room();
        assert!(map.sector_contains(0, Vec2::new(2.0, 2.0)));
        assert!(!map.sector_contains(0, Vec2::new(5.0, 5.0))); // inside the pillar
        assert!(!map.sector_contains(0, Vec2::new(11.0, 5.0)));
    }

    #[test]
    fn find_sector_uses_hint_neighbours_and_fallback() {
        let map = two_rooms(0.0, 3.0);
        assert_eq!(map.find_sector(Vec2::new(6.0, 2.0), Some(0)), Some(1));
        assert_eq!(map.find_sector(Vec2::new(2.0, 2.0), None), Some(0));
        assert_eq!(map.find_sector(Vec2::new(20.0, 2.0), Some(0)), None);
    }

    #[test]
    fn point_on_shared_edge_finds_a_sector() {
        let map = two_rooms(0.0, 3.0);
        // Exactly on the portal line, or on the shared vertex at the bottom of it: the half-open
        // rule assigns these to B, so the player never falls out of the world there.
        for p in [Vec2::new(4.0, 2.0), Vec2::new(4.0, 0.0)] {
            assert_eq!(map.find_sector(p, Some(0)), Some(1), "{p}");
            assert_eq!(map.find_sector(p, None), Some(1), "{p}");
        }
    }

    #[test]
    fn neighbours_lists_portal_sectors() {
        let map = crate::fixtures::door_rooms("(kind: Door)", "");
        let mut n: Vec<_> = map.neighbours(1).collect();
        n.sort();
        assert_eq!(n, vec![0, 2]);
        assert_eq!(map.neighbours(0).collect::<Vec<_>>(), vec![1]);
    }
}
