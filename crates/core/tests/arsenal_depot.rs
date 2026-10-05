use rr_core::map::{ActorKind, ItemKind, Key, Map, MoverKind};
use rr_core::validate::{Severity, validate};

const DEPOT: &str = include_str!("../../../assets/levels/arsenal_depot.ron");

fn depot() -> Map {
    Map::from_ron(DEPOT).expect("arsenal_depot.ron parses")
}

/// The sector holding `pos`.
fn sector_at(map: &Map, pos: rr_core::glam::Vec2) -> usize {
    map.find_sector(pos, None).expect("inside the map")
}

#[test]
fn arsenal_depot_validates() {
    let map = depot();
    let issues = validate(&map);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    // The only warnings: the secret room's loot sits behind the crack wall, which validate
    // treats as sealed (no level may depend on a blast).
    let secret = (0..map.sectors.len())
        .find(|&s| map.sectors[s].secret)
        .unwrap();
    let hidden: Vec<usize> = (0..map.items.len())
        .filter(|&i| sector_at(&map, map.items[i].pos) == secret)
        .collect();
    assert_eq!(hidden.len(), 2, "atom and rockets in the secret room");
    let expected: Vec<String> = hidden
        .iter()
        .map(|&i| {
            format!(
                "warning: item {i} ({:?}) cannot be reached from the start",
                map.items[i].kind
            )
        })
        .collect();
    let got: Vec<String> = issues.iter().map(|i| i.to_string()).collect();
    assert_eq!(got, expected);
}

#[test]
fn arsenal_depot_has_every_new_kind() {
    let map = depot();
    let actors = |k: ActorKind| map.actors.iter().filter(|a| a.kind == k).count();
    for k in [
        ActorKind::Grunt,
        ActorKind::Enforcer,
        ActorKind::Slasher,
        ActorKind::Drone,
        ActorKind::Barrel,
    ] {
        assert!(actors(k) >= 1, "no {k:?}");
    }
    assert_eq!(actors(ActorKind::Barrel), 5);
    assert_eq!(actors(ActorKind::Drone), 3);

    let items = |k: ItemKind| map.items.iter().filter(|i| i.kind == k).count();
    for k in [
        ItemKind::Chaingun,
        ItemKind::RocketLauncher,
        ItemKind::Rockets,
        ItemKind::PipeBombs,
        ItemKind::Armour,
        ItemKind::Medkit,
        ItemKind::Atom,
        ItemKind::Jetpack,
        ItemKind::NightVision,
    ] {
        assert!(items(k) >= 1, "no {k:?}");
    }
    assert_eq!(items(ItemKind::Key(Key::Blue)), 1);

    assert_eq!(map.sectors.iter().filter(|s| s.secret).count(), 1);
    assert!(
        map.walls.iter().filter(|w| w.glass).count() >= 2,
        "at least one glass portal (both sides)"
    );
    assert!(
        map.sectors
            .iter()
            .any(|s| s.mover.is_some_and(|m| m.kind == MoverKind::Crack))
    );
    assert!(map.lights.iter().any(|l| l.breakable));
}

/// Drones hover 2.5 m up: their room needs a tall ceiling. Barrels sit close enough to chain
/// (blast radius 4 m) along the range.
#[test]
fn arsenal_depot_drones_fly_tall_and_barrels_chain() {
    let map = depot();
    for d in map.actors.iter().filter(|a| a.kind == ActorKind::Drone) {
        let s = &map.sectors[sector_at(&map, d.pos)];
        assert!(s.ceil_z - s.floor_z >= 6.0, "drone room too low");
    }
    let mut barrels: Vec<_> = map
        .actors
        .iter()
        .filter(|a| a.kind == ActorKind::Barrel)
        .map(|a| a.pos)
        .collect();
    barrels.sort_by(|a, b| a.x.total_cmp(&b.x));
    for pair in barrels.windows(2) {
        let gap = pair[0].distance(pair[1]);
        assert!((2.0..4.0).contains(&gap), "barrel gap {gap}");
    }
}

/// The blue key sits on the ledge 8 m above the shaft floor, and a lift reaches it on foot.
#[test]
fn arsenal_depot_key_ledge_has_a_lift() {
    let map = depot();
    let key = map
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Key(Key::Blue))
        .unwrap();
    let ledge = sector_at(&map, key.pos);
    assert!(map.sectors[ledge].floor_z >= 8.0);
    let lift = map.neighbours(ledge).find(|&n| {
        matches!(
            map.sectors[n].mover.map(|m| m.kind),
            Some(MoverKind::Lift { to }) if to == map.sectors[ledge].floor_z
        )
    });
    assert!(lift.is_some(), "no lift up to the ledge");
}

/// A pipe bomb at the foot of the scorched wall reaches the crack; one in the dock does not.
#[test]
fn arsenal_depot_pipe_bomb_reaches_the_crack() {
    use rr_core::defs::SplashDef;
    use rr_core::explosion::{Blast, sectors_in_reach};
    use rr_core::glam::{Vec2, Vec3};
    use rr_core::mechanics::Mechanics;
    use rr_core::projectile::Shooter;

    let mut map = depot();
    let _mech = Mechanics::new(&mut map);
    let crack = (0..map.sectors.len())
        .find(|&s| {
            map.sectors[s]
                .mover
                .is_some_and(|m| m.kind == MoverKind::Crack)
        })
        .unwrap();
    let blast = |at: Vec3| Blast {
        center: at,
        sector: sector_at(&map, Vec2::new(at.x, at.y)),
        splash: SplashDef {
            radius: 6.0,
            damage: 100,
            self_scale: 1.0,
        },
        owner: Shooter::Player,
    };
    assert_eq!(
        sectors_in_reach(&map, &blast(Vec3::new(44.5, 9.0, 0.2))),
        vec![crack]
    );
    assert!(sectors_in_reach(&map, &blast(Vec3::new(6.0, 5.0, 0.2))).is_empty());
}
