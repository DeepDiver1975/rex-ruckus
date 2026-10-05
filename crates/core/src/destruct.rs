//! Destructible world state: things shots and blasts break for good. For now that is glass
//! panes in portals (`Wall::glass`), crack walls and the secrets found.
//!
//! Every change to a wall or sector returns the sectors whose meshes must be rebuilt; the game
//! feeds them into the same dirty-sector path `Mechanics::tick` uses.

use crate::map::{Map, MoverKind, SectorId, WallId};
use crate::mechanics::{Mechanics, Motion};
use crate::trace::{Hit, HitKind, can_see, ray_sphere};
use glam::Vec3;

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

/// Radius (m) of a breakable light's fixture, centred on the light.
pub const FIXTURE_RADIUS: f32 = 0.2;

/// Runtime destruction state of one level. Lives in `Combat::destruct`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Destruct {
    /// Per light (index in `Map::lights`): the fixture has been broken.
    lights: Vec<bool>,
    /// Per sector: a crack wall that has been set off.
    cracks_open: Vec<bool>,
    /// Per sector: a secret the player has entered.
    secrets_found: Vec<bool>,
    secrets_total: u32,
}

impl Destruct {
    pub fn new(map: &Map) -> Destruct {
        Destruct {
            lights: vec![false; map.lights.len()],
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
        mech.emit_started(m);
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

    /// Breaks light `i`'s fixture for good. False if the light does not exist, is not
    /// breakable or is already broken.
    pub fn break_light(&mut self, map: &Map, i: usize) -> bool {
        match (map.lights.get(i), self.lights.get_mut(i)) {
            (Some(l), Some(broken)) if l.breakable && !*broken => {
                *broken = true;
                true
            }
            _ => false,
        }
    }

    /// Whether light `i` has been broken.
    pub fn light_broken(&self, i: usize) -> bool {
        self.lights.get(i).copied().unwrap_or(false)
    }

    /// The nearest intact breakable fixture the ray `origin + t·dir` (`dir` unit length) meets
    /// before distance `max`, with its distance.
    pub fn fixture_hit(
        &self,
        map: &Map,
        origin: Vec3,
        dir: Vec3,
        max: f32,
    ) -> Option<(usize, f32)> {
        map.lights
            .iter()
            .enumerate()
            .filter(|&(i, l)| l.breakable && !self.light_broken(i))
            .filter_map(|(i, l)| {
                let c = Vec3::new(l.pos.0, l.pos.1, l.pos.2);
                ray_sphere(origin, dir, c, FIXTURE_RADIUS).map(|t| (i, t))
            })
            .filter(|&(_, t)| t < max)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Intact breakable lights a blast at `center` (in `sector`) reaches: the fixture centre
    /// within `radius` and in the world's line of sight from the centre. Ascending.
    pub fn lights_in_reach(
        &self,
        map: &Map,
        center: Vec3,
        sector: SectorId,
        radius: f32,
    ) -> Vec<usize> {
        map.lights
            .iter()
            .enumerate()
            .filter(|&(i, l)| l.breakable && !self.light_broken(i))
            .filter(|(_, l)| {
                let c = Vec3::new(l.pos.0, l.pos.1, l.pos.2);
                c.distance(center) <= radius && can_see(map, center, sector, c)
            })
            .map(|(i, _)| i)
            .collect()
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
    use crate::movement::{Pass, Tuning};
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
    fn open_crack_emits_started_then_stopped() {
        use crate::fixtures::door_rooms;
        use crate::map::MoverKind;
        use crate::mechanics::{MechEvent, Mechanics};
        let mut map = door_rooms("(kind: Crack)", "");
        let mut mech = Mechanics::new(&mut map);
        let mut dest = Destruct::new(&map);
        assert!(dest.open_crack(&mut mech, 1));
        assert_eq!(
            mech.drain_events(),
            vec![MechEvent::MoverStarted {
                sector: 1,
                kind: MoverKind::Crack
            }]
        );
        for _ in 0..600 {
            mech.tick(&mut map, &mut [], 0.05);
        }
        assert_eq!(
            mech.drain_events(),
            vec![MechEvent::MoverStopped {
                sector: 1,
                kind: MoverKind::Crack
            }]
        );
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
        assert_eq!(next_hop(&map, 1, 0, Pass::Walk(t)), None);
        assert_eq!(next_hop(&map, 0, 1, Pass::Walk(t)), None);
        let w = pane(&map);
        Destruct::new(&map).break_glass(&mut map, w);
        assert_eq!(next_hop(&map, 1, 0, Pass::Walk(t)), Some(0));
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
