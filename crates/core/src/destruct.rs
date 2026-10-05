//! Destructible world state: things shots and blasts break for good. For now that is glass
//! panes in portals (`Wall::glass`), crack walls and the secrets found.
//!
//! Every change to a wall or sector returns the sectors whose meshes must be rebuilt; the game
//! feeds them into the same dirty-sector path `Mechanics::tick` uses.

use crate::map::{Map, MoverKind, SectorId, WallId};
use crate::mechanics::{Mechanics, Motion};
use crate::trace::{Hit, HitKind};

/// Whether `hit` landed on an intact glass pane itself: a wall hit on a glass portal at a
/// height inside its opening (the higher floor to the lower ceiling of the two sectors). A hit
/// on the solid sill or soffit around the pane is an ordinary wall hit.
pub fn hits_pane(map: &Map, hit: &Hit) -> bool {
    let HitKind::Wall(w) = hit.kind else {
        return false;
    };
    let wall = &map.walls[w];
    let Some(far) = wall.next_sector.filter(|_| wall.glass) else {
        return false;
    };
    let (near, far) = (&map.sectors[wall.sector], &map.sectors[far]);
    let (lo, hi) = (near.floor_z.max(far.floor_z), near.ceil_z.min(far.ceil_z));
    (lo..=hi).contains(&hit.point.z)
}

/// Metres per second a cracked wall opens at.
pub const CRACK_SPEED: f32 = 6.0;

/// Runtime destruction state of one level. Lives in `Combat::destruct`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Destruct {
    /// Per sector: a crack wall that has been set off.
    cracks_open: Vec<bool>,
    /// Per sector: a secret the player has entered.
    secrets_found: Vec<bool>,
    secrets_total: u32,
}

impl Destruct {
    pub fn new(map: &Map) -> Destruct {
        Destruct {
            cracks_open: vec![false; map.sectors.len()],
            secrets_found: vec![false; map.sectors.len()],
            secrets_total: map.sectors.iter().filter(|s| s.secret).count() as u32,
        }
    }

    /// Sets crack wall `s` opening, fast and for good: no auto-return, and nothing closes it
    /// again. The ceiling then moves through `Mechanics::tick`, which reports the sector as
    /// changed like any door. False if `s` is no crack or was already set off.
    pub fn open_crack(&mut self, mech: &mut Mechanics, s: SectorId) -> bool {
        let Some(m) = mech.mover_in(s) else {
            return false;
        };
        let mv = &mut mech.movers[m];
        if mv.def.kind != MoverKind::Crack || self.cracks_open.get(s) != Some(&false) {
            return false;
        }
        self.cracks_open[s] = true;
        mv.def.speed = CRACK_SPEED;
        mv.def.auto_return = None;
        mv.motion = Motion::ToEnd;
        true
    }

    /// The player is in sector `s`. True the first time that is a secret sector.
    pub fn enter_sector(&mut self, map: &Map, s: SectorId) -> bool {
        let secret = map.sectors.get(s).is_some_and(|sec| sec.secret);
        match self.secrets_found.get_mut(s) {
            Some(found) if secret && !*found => {
                *found = true;
                true
            }
            _ => false,
        }
    }

    /// Secrets found so far and in the level.
    pub fn secrets(&self) -> (u32, u32) {
        let found = self.secrets_found.iter().filter(|&&f| f).count() as u32;
        (found, self.secrets_total)
    }

    /// Shatters the glass pane in portal `w` (either side). Clears `glass` on both sides and
    /// returns both sectors (this wall's first), whose meshes lose the pane. A wall without
    /// intact glass changes nothing and returns no sectors.
    pub fn break_glass(&mut self, map: &mut Map, w: WallId) -> Vec<SectorId> {
        let wall = &map.walls[w];
        let (Some(back), Some(far)) = (wall.next_wall, wall.next_sector) else {
            return Vec::new();
        };
        if !wall.glass {
            return Vec::new();
        }
        let near = wall.sector;
        map.walls[w].glass = false;
        map.walls[back].glass = false;
        vec![near, far]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::next_hop;
    use crate::collide::{Body, clip_move};
    use crate::combat::Combat;
    use crate::defs::Defs;
    use crate::fixtures::{defs, glass_rooms};
    use crate::map::{ActorKind, ActorSpawn};
    use crate::movement::Tuning;
    use crate::trace::{HitKind, Ray, can_see, trace_world};
    use glam::{Vec2, Vec3};

    /// The A side of the pane in `glass_rooms` (sector 0, at x = 10).
    fn pane(map: &Map) -> WallId {
        (0..map.walls.len())
            .find(|&w| map.walls[w].glass && map.walls[w].sector == 0)
            .expect("glass_rooms has a pane")
    }

    fn east_ray(map: &Map, from: Vec3) -> Ray {
        Ray {
            origin: from,
            dir: Vec3::X,
            sector: map.find_sector(from.truncate(), None).unwrap(),
            max: 50.0,
        }
    }

    /// A sleeping Grunt in the booth, behind the pane.
    fn sleeper_behind_glass(map: &mut Map, d: &Defs) -> Combat {
        map.actors.push(ActorSpawn {
            kind: ActorKind::Grunt,
            pos: Vec2::new(12.0, 5.0),
            angle: 0.0,
            asleep: true,
        });
        Combat::spawn(map, d, 1)
    }

    #[test]
    fn glass_blocks_move_shot_sight_noise() {
        let d = defs();
        let mut map = glass_rooms();
        let w = pane(&map);

        // Movement: walking east into the pane stops a radius short of it.
        let mut body = Body::spawn(&map, Vec2::new(9.0, 5.0), 0.35, 1.8).unwrap();
        clip_move(&map, &mut body, Vec2::new(2.0, 0.0), 0.55);
        assert_eq!(body.sector, 0);
        assert!(body.pos.x <= 10.0 - 0.35 + 1e-3, "{:?}", body.pos);

        // Shots: a ray inside the opening hits the pane itself.
        let eye = Vec3::new(8.0, 5.0, 1.6);
        let h = trace_world(&map, &east_ray(&map, eye)).unwrap();
        assert_eq!(h.kind, HitKind::Wall(w));
        assert!((h.point.x - 10.0).abs() < 1e-4);

        // Sight: nothing is seen through it.
        let booth = Vec3::new(12.0, 5.0, 1.6);
        assert!(!can_see(&map, eye, 0, booth));

        // Sound: a gunshot in the hall does not wake the sleeper in the booth.
        let mut c = sleeper_behind_glass(&mut map, &d);
        assert!(c.make_noise(&map, eye, 0, 40.0).is_empty());

        // Once broken, all four pass.
        Destruct::new(&map).break_glass(&mut map, w);
        let mut body = Body::spawn(&map, Vec2::new(9.0, 5.0), 0.35, 1.8).unwrap();
        clip_move(&map, &mut body, Vec2::new(2.0, 0.0), 0.55);
        assert_eq!(body.sector, 1);
        let h = trace_world(&map, &east_ray(&map, eye)).unwrap();
        assert!((h.point.x - 14.0).abs() < 1e-4, "{h:?}");
        assert!(can_see(&map, eye, 0, booth));
        assert_eq!(c.make_noise(&map, eye, 0, 40.0), vec![0]);
    }

    #[test]
    fn glass_blocks_actor_path() {
        let mut map = glass_rooms();
        let t = Tuning::default();
        assert_eq!(next_hop(&map, 1, 0, &t), None);
        assert_eq!(next_hop(&map, 0, 1, &t), None);
        let w = pane(&map);
        Destruct::new(&map).break_glass(&mut map, w);
        assert_eq!(next_hop(&map, 1, 0, &t), Some(0));
    }

    #[test]
    fn break_glass_dirties_both_sectors() {
        let mut map = glass_rooms();
        let w = pane(&map);
        let back = map.walls[w].next_wall.unwrap();
        let mut d = Destruct::new(&map);
        // Either side breaks the whole pane.
        assert_eq!(d.break_glass(&mut map, back), vec![1, 0]);
        assert!(!map.walls[w].glass && !map.walls[back].glass);
        assert_eq!(map.walls[w].passage(), Some(1));
        // Nothing left to break.
        assert!(d.break_glass(&mut map, w).is_empty());
        // A wall that never held glass.
        assert!(d.break_glass(&mut map, 0).is_empty());
    }
}
