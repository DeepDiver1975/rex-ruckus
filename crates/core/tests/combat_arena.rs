use rr_core::actors::AiState;
use rr_core::collide::Body;
use rr_core::combat::{Combat, CombatEvent, PlayerTarget, level_seed};
use rr_core::defs::{Defs, WeaponId};
use rr_core::glam::{Vec2, Vec3};
use rr_core::map::{ActorKind, ItemKind, Key, Map};
use rr_core::mechanics::Mechanics;
use rr_core::validate::validate;
use rr_core::vitals::Vitals;
use rr_core::weapons::{Arsenal, WeaponEvent, WeaponInput};

const ARENA: &str = include_str!("../../../assets/levels/combat_arena.ron");
const DT: f32 = 1.0 / 60.0;

#[test]
fn arena_validates_cleanly() {
    let map = Map::from_ron(ARENA).expect("combat_arena.ron parses");
    assert_eq!(validate(&map), vec![]);
}

#[test]
fn arena_has_six_grunts_and_required_items() {
    let map = Map::from_ron(ARENA).unwrap();
    assert_eq!(map.actors.len(), 6);
    assert!(map.actors.iter().all(|a| a.kind == ActorKind::Grunt));
    let awake = map.actors.iter().filter(|a| !a.asleep).count();
    assert_eq!(awake, 3, "1 awake in the arena, 2 on the balcony");
    let count = |k: ItemKind| map.items.iter().filter(|i| i.kind == k).count();
    assert_eq!(count(ItemKind::Shotgun), 1);
    assert!(count(ItemKind::PistolAmmo) >= 2);
    assert!(count(ItemKind::ShotgunShells) >= 2);
    assert!(count(ItemKind::HealthSmall) >= 2);
    assert_eq!(count(ItemKind::Key(Key::Blue)), 1);
    assert!((4..=6).contains(&map.lights.len()), "{}", map.lights.len());
}

/// The awake arena Grunt: the one awake actor on the arena floor (z = 0).
fn awake_arena_grunt(map: &Map) -> usize {
    let found: Vec<usize> = (0..map.actors.len())
        .filter(|&i| {
            let a = map.actors[i];
            let s = map.find_sector(a.pos, None).unwrap();
            !a.asleep && map.sectors[s].floor_z == 0.0
        })
        .collect();
    assert_eq!(found.len(), 1, "exactly one awake Grunt on the arena floor");
    found[0]
}

/// Core-only fight: the player stands inside the arena door and holds fire with the pistol,
/// aimed at the awake arena Grunt's chest every tick, until it is dead.
#[test]
fn scripted_arena_fight() {
    let mut map = Map::from_ron(ARENA).unwrap();
    let mut mech = Mechanics::new(&mut map);
    let defs = Defs::builtin();
    let s = map.player_start.pos;
    let mut body = Body::spawn(&map, Vec2::new(s.0, s.1), 0.35, 1.8).unwrap();
    let mut combat = Combat::spawn(&map, &defs, level_seed(&map.name));
    let target = awake_arena_grunt(&map);
    assert_eq!(combat.actors.len(), 6);

    // Step through the arena door into the arena (a teleport; the door itself is not needed).
    body = Body::spawn(&map, Vec2::new(5.0, 10.0), body.radius, body.height).unwrap();
    assert_eq!(map.sectors[body.sector].floor_z, 0.0);

    let mut vitals = Vitals::new();
    let mut arsenal = Arsenal::new(&defs);
    assert_eq!(arsenal.current, WeaponId::Pistol);
    let mut shots = 0;
    let mut dead_at = None;
    for tick in 0..600 {
        let eye = body.pos + Vec3::Z * 0.9 * body.height;
        let t = &combat.actors[target];
        let chest = t.body.pos + Vec3::Z * 0.6 * t.body.height;
        let aim = (chest - eye).normalize();
        let input = WeaponInput {
            fire: t.alive(),
            ..Default::default()
        };
        for ev in arsenal.tick(&defs, &input, aim, &mut combat.rng, DT) {
            if matches!(ev, WeaponEvent::Fire { .. }) {
                shots += 1;
            }
            combat.player_attack(&mut map, &defs, eye, body.sector, &ev);
        }
        let mut player = PlayerTarget {
            body: &mut body,
            vitals: &mut vitals,
            eye,
        };
        let events = combat.tick(&mut map, &mut mech, &defs, &mut player, DT);
        assert!(!events.contains(&CombatEvent::PlayerKilled), "player died");
        let (idx, mut bodies) = combat.living_bodies();
        mech.tick(&mut map, &mut bodies, DT);
        combat.write_back(&idx, &bodies);
        if combat.actors[target].state == AiState::Dead {
            dead_at = Some(tick);
            break;
        }
    }
    let dead_at = dead_at.expect("the awake arena Grunt dies within 600 ticks");
    assert!(dead_at < 600);
    assert!(
        map.find_sector(body.pos.truncate(), None).is_some(),
        "player left the map: {}",
        body.pos
    );
    let hp = vitals.health.hp;
    assert!(hp > 0, "player survived with {hp} hp");
    assert!((3..=24).contains(&shots), "{shots} shots");
}
