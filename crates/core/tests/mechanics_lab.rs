use rr_core::collide::Body;
use rr_core::glam::{Vec2, Vec3};
use rr_core::interact::use_target;
use rr_core::map::{ItemKind, Key, KeySet, Map};
use rr_core::mechanics::{Mechanics, UseOutcome};
use rr_core::movement::{MoveInput, Tuning, step_player};
use rr_core::validate::validate;

const LAB: &str = include_str!("../../../assets/levels/mechanics_lab.ron");
const DT: f32 = 1.0 / 60.0;

#[test]
fn lab_validates_cleanly() {
    let map = Map::from_ron(LAB).expect("mechanics_lab.ron parses");
    assert_eq!(validate(&map), vec![]);
}

/// Core-only playthrough: player movement, movers and pickups, ticked the way the game ticks them.
struct Run {
    map: Map,
    mech: Mechanics,
    body: Body,
    keys: KeySet,
}

impl Run {
    fn new() -> Run {
        let mut map = Map::from_ron(LAB).unwrap();
        let mech = Mechanics::new(&mut map);
        let s = map.player_start.pos;
        let body = Body::spawn(&map, Vec2::new(s.0, s.1), 0.35, 1.8).unwrap();
        Run {
            map,
            mech,
            body,
            keys: KeySet::default(),
        }
    }

    fn tick(&mut self, wish: Vec2) {
        let input = MoveInput {
            wish,
            ..Default::default()
        };
        step_player(&self.map, &mut self.body, &input, &Tuning::default(), DT);
        let mut bodies = [self.body];
        self.mech.tick(&mut self.map, &mut bodies, DT);
        self.body = bodies[0];
        for i in self.mech.pickup(&self.map, &self.body) {
            if let ItemKind::Key(k) = self.map.items[i].kind {
                self.keys.insert(k);
            }
        }
    }

    fn wait(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.tick(Vec2::ZERO);
        }
    }

    /// Walks straight at (x, y) and brakes on arrival (a test harness, not a player).
    fn walk_to(&mut self, x: f32, y: f32) {
        let target = Vec2::new(x, y);
        for _ in 0..600 {
            let to = target - self.body.pos.truncate();
            if to.length() < 0.2 {
                self.body.vel = Vec3::ZERO;
                return;
            }
            self.tick(to.normalize());
        }
        panic!("stuck at {} walking to {target}", self.body.pos);
    }

    fn press_use(&mut self, heading_deg: f32) -> UseOutcome {
        let t = use_target(&self.map, &self.mech, &self.body, heading_deg.to_radians())
            .unwrap_or_else(|| panic!("nothing to use at {} facing {heading_deg}°", self.body.pos));
        self.mech.activate(&self.map, t, self.keys)
    }
}

#[test]
fn lab_can_be_finished() {
    let mut r = Run::new();
    // Hall switch opens the closet door; fetch the blue key.
    r.walk_to(1.0, 5.0);
    assert_eq!(r.press_use(180.0), UseOutcome::Activated);
    r.wait(90);
    r.walk_to(11.0, 5.0);
    r.walk_to(14.5, 5.0);
    assert!(r.keys.contains(Key::Blue));
    // Through D1 into the corridor; D2 is red-locked.
    r.walk_to(11.0, 5.0);
    r.walk_to(6.0, 9.0);
    assert_eq!(r.press_use(90.0), UseOutcome::Activated);
    r.wait(80);
    r.walk_to(6.0, 12.0);
    r.walk_to(6.0, 17.0);
    assert_eq!(r.press_use(90.0), UseOutcome::NeedKey(Key::Red));
    // Lift up to the balcony for the red key, then back down.
    r.walk_to(6.0, 13.0);
    r.walk_to(9.0, 13.0);
    assert_eq!(r.press_use(0.0), UseOutcome::Activated);
    r.wait(90);
    assert!((r.body.pos.z - 2.4).abs() < 1e-4, "rode up: {}", r.body.pos);
    r.walk_to(12.0, 13.0);
    assert!(r.keys.contains(Key::Red));
    r.walk_to(9.0, 13.0);
    assert_eq!(r.press_use(0.0), UseOutcome::Activated);
    r.wait(90);
    assert!(r.body.pos.z.abs() < 1e-4, "rode down: {}", r.body.pos);
    // Red door, vault, exit.
    r.walk_to(6.0, 13.0);
    r.walk_to(6.0, 17.0);
    assert_eq!(r.press_use(90.0), UseOutcome::Activated);
    r.wait(80);
    r.walk_to(6.0, 23.0);
    assert_eq!(r.press_use(90.0), UseOutcome::Exit);
}

#[test]
fn switch_only_covers_its_short_wall_section() {
    let mut r = Run::new();
    r.walk_to(1.0, 8.5);
    let t = use_target(&r.map, &r.mech, &r.body, 180f32.to_radians());
    assert!(t.is_none(), "west wall at y=8.5 is plain wall: {t:?}");
}
