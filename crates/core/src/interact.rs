//! What the use key points at (Build's `neartag`).

use crate::collide::Body;
use crate::map::Map;
use crate::mechanics::{Mechanics, UseTarget};
use crate::trace::wall_t;
use glam::Vec2;

pub const USE_RANGE: f32 = 1.6;
/// Eye height as a fraction of body height, for deciding whether the use ray fits through a portal.
const EYE_FRACTION: f32 = 0.9;

/// What pressing use would operate for `body` looking along `heading` (core radians).
pub fn use_target(map: &Map, mech: &Mechanics, body: &Body, heading: f32) -> Option<UseTarget> {
    let origin = body.pos.truncate();
    let dir = Vec2::new(heading.cos(), heading.sin());
    let eye = body.pos.z + body.height * EYE_FRACTION;
    let manual = |m: usize| mech.movers[m].def.channel.is_none();

    let mut hits: Vec<(f32, usize)> = map
        .walls
        .iter()
        .enumerate()
        .filter_map(|(id, w)| {
            wall_t(origin, dir, w)
                .filter(|&(t, u)| (0.0..=USE_RANGE).contains(&t) && (0.0..=1.0).contains(&u))
                .map(|(t, _)| t)
                .map(|t| (t, id))
        })
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (_, wid) in hits {
        if let Some(i) = map.switches.iter().position(|s| s.wall == wid) {
            return Some(UseTarget::Switch(i));
        }
        let Some(next) = map.walls[wid].passage() else {
            break;
        };
        if let Some(m) = mech.mover_in(next) {
            if manual(m) {
                return Some(UseTarget::Mover(m));
            }
            break;
        }
        let far = &map.sectors[next];
        if !(far.floor_z < eye && eye < far.ceil_z) {
            break;
        }
    }
    mech.mover_in(body.sector)
        .filter(|&m| manual(m))
        .map(UseTarget::Mover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{door_rooms, lift_shaft};
    use glam::Vec2;
    use std::f32::consts::PI;

    fn at(map: &Map, x: f32, y: f32) -> Body {
        Body::spawn(map, Vec2::new(x, y), 0.35, 1.8).unwrap()
    }

    #[test]
    fn facing_a_closed_door_targets_it() {
        let mut map = door_rooms("(kind: Door)", "");
        let mech = Mechanics::new(&mut map);
        assert_eq!(
            use_target(&map, &mech, &at(&map, 3.0, 2.0), 0.0),
            Some(UseTarget::Mover(0))
        );
    }

    #[test]
    fn out_of_range_or_facing_away_finds_nothing() {
        let mut map = door_rooms("(kind: Door)", "");
        let mech = Mechanics::new(&mut map);
        assert_eq!(
            use_target(&map, &mech, &at(&map, 2.0, 2.0), 0.0),
            None,
            "2 m away"
        );
        assert_eq!(
            use_target(&map, &mech, &at(&map, 3.0, 2.0), PI),
            None,
            "facing away"
        );
    }

    #[test]
    fn switch_on_wall_is_targeted_and_remote_door_is_not() {
        let mut map = door_rooms(
            "(kind: Door, channel: Some(1))",
            "switches: [(wall: (7, 0), action: Channel(1))],",
        );
        let mech = Mechanics::new(&mut map);
        assert_eq!(
            use_target(&map, &mech, &at(&map, 1.0, 2.0), PI),
            Some(UseTarget::Switch(0))
        );
        assert_eq!(use_target(&map, &mech, &at(&map, 3.0, 2.0), 0.0), None);
    }

    #[test]
    fn standing_on_a_lift_targets_it() {
        let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mech = Mechanics::new(&mut map);
        // The ray meets the ledge face (no opening at eye height), so the fallback picks the lift.
        assert_eq!(
            use_target(&map, &mech, &at(&map, 5.0, 2.0), 0.0),
            Some(UseTarget::Mover(0))
        );
    }

    #[test]
    fn raised_lift_is_targeted_from_below() {
        let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mut mech = Mechanics::new(&mut map);
        mech.toggle(0);
        for _ in 0..120 {
            mech.tick(&mut map, &mut [], 1.0 / 60.0);
        }
        assert_eq!(map.sectors[1].floor_z, 2.0);
        assert_eq!(
            use_target(&map, &mech, &at(&map, 3.5, 2.0), 0.0),
            Some(UseTarget::Mover(0))
        );
    }
}
