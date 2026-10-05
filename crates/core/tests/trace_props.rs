mod common;

use proptest::prelude::*;
use rr_core::fixtures::combat_room;
use rr_core::geom::closest_point_on_segment;
use rr_core::glam::{Vec2, Vec3};
use rr_core::map::{Map, SectorId};
use rr_core::mechanics::Mechanics;
use rr_core::trace::{Ray, can_see, trace_world};

const TOL: f32 = 1e-3;

fn map(doors_closed: bool) -> Map {
    let mut map = combat_room();
    if doors_closed {
        Mechanics::new(&mut map);
    }
    map
}

/// Distance from `p` to the nearest wall of sector `s`.
fn wall_dist(map: &Map, s: SectorId, p: Vec2) -> f32 {
    map.sectors[s]
        .walls()
        .map(|w| {
            let w = &map.walls[w];
            closest_point_on_segment(p, w.a, w.b).distance(p)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The sector strictly containing `p` (at least `margin` from all its walls), if any.
fn interior_sector(map: &Map, p: Vec2, margin: f32) -> Option<SectorId> {
    map.find_sector(p, None)
        .filter(|&s| wall_dist(map, s, p) > margin)
}

/// `p` is inside sector `s`, or within `TOL` of one of its edges, and within its z range.
fn in_or_on(map: &Map, s: SectorId, p: Vec3) -> bool {
    let sec = &map.sectors[s];
    let xy = p.truncate();
    (map.sector_contains(s, xy) || wall_dist(map, s, xy) <= TOL)
        && sec.floor_z - TOL <= p.z
        && p.z <= sec.ceil_z + TOL
}

fn xy() -> impl Strategy<Value = Vec2> {
    (0.0f32..16.5, 0.0f32..8.0).prop_map(|(x, y)| Vec2::new(x, y))
}

/// A unit direction, uniform over the sphere.
fn dir() -> impl Strategy<Value = Vec3> {
    (0.0f32..std::f32::consts::TAU, -1.0f32..=1.0).prop_map(|(a, z)| {
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z).normalize()
    })
}

/// Origins are rejection-sampled (about a quarter of the bounding box is pillar, void or too
/// close to a wall), so the reject budget scales with the case count instead of proptest's fixed
/// default of 1024.
fn config() -> ProptestConfig {
    let cases = common::cases(256);
    ProptestConfig {
        max_global_rejects: cases.saturating_mul(4),
        ..ProptestConfig::with_cases(cases)
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn trace_never_escapes_map(
        doors_closed in any::<bool>(),
        p in xy(),
        frac in 0.01f32..0.99,
        d in dir(),
    ) {
        let map = map(doors_closed);
        let s = interior_sector(&map, p, TOL);
        prop_assume!(s.is_some());
        let s = s.unwrap();
        let sec = &map.sectors[s];
        prop_assume!(sec.ceil_z - sec.floor_z > 0.01);
        let origin = p.extend(sec.floor_z + frac * (sec.ceil_z - sec.floor_z));
        let ray = Ray { origin, dir: d, sector: s, max: 1000.0 };

        let h = trace_world(&map, &ray);
        prop_assert!(h.is_some(), "a closed map always stops the ray: {:?}", ray);
        let h = h.unwrap();
        prop_assert!(
            (0..map.sectors.len()).any(|t| in_or_on(&map, t, h.point)),
            "hit {:?} outside every sector (ray {:?})", h, ray
        );
        prop_assert!(in_or_on(&map, h.sector, h.point), "hit {:?} not in its sector", h);
        prop_assert!((h.point - (origin + d * h.dist)).length() < TOL, "{:?}", h);
    }

    #[test]
    fn can_see_is_symmetric_on_open_ground(
        doors_closed in any::<bool>(),
        a in xy(),
        b in xy(),
        chest in 0.9f32..1.6,
    ) {
        let map = map(doors_closed);
        // Keep clear of walls so the 5 cm end margin cannot make one direction stop short of a
        // wall the other direction starts against.
        let (sa, sb) = (interior_sector(&map, a, 0.1), interior_sector(&map, b, 0.1));
        prop_assume!(sa.is_some() && sb.is_some());
        let (sa, sb) = (sa.unwrap(), sb.unwrap());
        let a3 = a.extend(map.sectors[sa].floor_z + chest);
        let b3 = b.extend(map.sectors[sb].floor_z + chest);
        prop_assume!(a3.z < map.sectors[sa].ceil_z - 0.1 && b3.z < map.sectors[sb].ceil_z - 0.1);
        prop_assert_eq!(
            can_see(&map, a3, sa, b3),
            can_see(&map, b3, sb, a3),
            "{} (sector {}) vs {} (sector {})", a3, sa, b3, sb
        );
    }
}
