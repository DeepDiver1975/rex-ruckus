use bevy::prelude::*;
use rr_core::defs::{Defs, WeaponId};
use rr_core::fixtures::{combat_room, door_rooms, glass_rooms, lift_shaft};
use rr_core::map::{ActorKind, ActorSpawn, Map};
use rr_core::mechanics::Motion;
use rr_core::weapons::{WeaponEvent, WeaponPhase};
use rr_game::combat::{
    CombatSimPlugin, FxQueue, LevelCombat, PlayerArsenal, PlayerVitals, insert_defs,
};
use rr_game::flow::FlowPlugin;
use rr_game::level::CurrentMap;
use rr_game::mechanics::{
    DirtySectors, HudMessage, LevelMechanics, MechanicsSimPlugin, insert_level,
};
use rr_game::player::{
    EYE_BELOW_TOP, Inventory, Look, PendingInput, PlayerBody, PlayerSimPlugin, fire_gate,
};

fn app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
    ));
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn input(app: &mut App) -> Mut<'_, PendingInput> {
    let mut q = app.world_mut().query::<&mut PendingInput>();
    q.single_mut(app.world_mut()).unwrap()
}

fn arsenal(app: &mut App) -> Mut<'_, PlayerArsenal> {
    let mut q = app.world_mut().query::<&mut PlayerArsenal>();
    q.single_mut(app.world_mut()).unwrap()
}

fn health(app: &mut App) -> Mut<'_, PlayerVitals> {
    let mut q = app.world_mut().query::<&mut PlayerVitals>();
    q.single_mut(app.world_mut()).unwrap()
}

fn shots(app: &App) -> usize {
    app.world()
        .resource::<FxQueue>()
        .weapon
        .iter()
        .filter(|e| matches!(e, WeaponEvent::Fire { .. }))
        .count()
}

fn take_fx(app: &mut App) -> FxQueue {
    std::mem::take(&mut *app.world_mut().resource_mut::<FxQueue>())
}

#[test]
fn grab_click_never_fires() {
    let mut armed = false;
    // Click while not grabbed (this click grabs the cursor): no fire.
    assert!(!fire_gate(false, true, &mut armed));
    // Next frames: grabbed, the grabbing click is still held: still no fire.
    assert!(!fire_gate(true, true, &mut armed));
    assert!(!fire_gate(true, true, &mut armed));
    // Released while grabbed: arms; the next press fires and keeps firing while held.
    assert!(!fire_gate(true, false, &mut armed));
    assert!(fire_gate(true, true, &mut armed));
    assert!(fire_gate(true, true, &mut armed));
    // Losing the grab disarms again; regrabbing with the button down does not fire.
    assert!(!fire_gate(false, true, &mut armed));
    assert!(!fire_gate(true, true, &mut armed));
    assert!(!fire_gate(true, false, &mut armed));
    assert!(fire_gate(true, true, &mut armed));
}

#[test]
fn player_starts_with_full_health_and_the_pistol() {
    let mut app = app(combat_room());
    assert_eq!(health(&mut app).0.health.hp, 100);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::Pistol);
}

#[test]
fn fire_tap_between_ticks_fires_once() {
    let mut app = app(combat_room());
    let clip = arsenal(&mut app).0.clip[WeaponId::Pistol.index()];
    // A tap: pressed and released before the tick ran, so `fire` (held) is false.
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    assert!(!input(&mut app).fire_pressed, "latch consumed by the tick");
    assert_eq!(shots(&app), 1);
    ticks(&mut app, 60);
    assert_eq!(shots(&app), 1, "a tap fires exactly once");
    assert_eq!(arsenal(&mut app).0.clip[WeaponId::Pistol.index()], clip - 1);
}

#[test]
fn holding_fire_respects_refire() {
    let mut app = app(combat_room());
    input(&mut app).fire = true;
    ticks(&mut app, 60);
    // Pistol refire 0.18 s: shots at 0, .18, .36, .54, .72, .90 within one second.
    assert_eq!(shots(&app), 6);
    input(&mut app).fire = false;
    ticks(&mut app, 60);
    assert_eq!(shots(&app), 6);
}

#[test]
fn reload_and_kick_are_latched() {
    let mut app = app(combat_room());
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 30);
    input(&mut app).reload = true;
    input(&mut app).kick = true;
    ticks(&mut app, 1);
    assert!(!input(&mut app).reload && !input(&mut app).kick);
    let fx = take_fx(&mut app);
    assert!(fx.weapon.contains(&WeaponEvent::ReloadStart));
    assert!(
        fx.weapon
            .iter()
            .any(|e| matches!(e, WeaponEvent::Kick { .. }))
    );
}

#[test]
fn pressing_2_then_wheel_switches_weapons() {
    let mut app = app(combat_room());
    input(&mut app).select = Some(WeaponId::Boot);
    ticks(&mut app, 30);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::Boot);
    // Key 2.
    input(&mut app).select = Some(WeaponId::Pistol);
    ticks(&mut app, 1);
    assert_eq!(input(&mut app).select, None, "selection consumed");
    ticks(&mut app, 29);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::Pistol);
    // Wheel up: next owned weapon after the pistol wraps to the boot (no shotgun yet).
    input(&mut app).cycle = 1;
    ticks(&mut app, 1);
    assert_eq!(input(&mut app).cycle, 0, "wheel consumed");
    assert!(matches!(
        arsenal(&mut app).0.phase,
        WeaponPhase::Switching {
            to: WeaponId::Boot,
            ..
        }
    ));
    ticks(&mut app, 29);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::Boot);
    // Wheel down: back to the pistol.
    input(&mut app).cycle = -1;
    ticks(&mut app, 30);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::Pistol);
    let switched: Vec<_> = take_fx(&mut app)
        .weapon
        .into_iter()
        .filter_map(|e| match e {
            WeaponEvent::Switched(w) => Some(w),
            _ => None,
        })
        .collect();
    assert_eq!(
        switched,
        [
            WeaponId::Boot,
            WeaponId::Pistol,
            WeaponId::Boot,
            WeaponId::Pistol
        ]
    );
}

/// Points the player's view at the actor's chest.
fn aim_at_actor(app: &mut App, actor: usize) {
    let target = {
        let b = app.world().resource::<LevelCombat>().0.actors[actor].body;
        b.pos + Vec3::Z * 0.6 * b.height
    };
    let mut q = app.world_mut().query::<(&PlayerBody, &mut Look)>();
    let (body, mut look) = q.single_mut(app.world_mut()).unwrap();
    let eye = body.0.pos + Vec3::Z * (body.0.height - EYE_BELOW_TOP);
    let d = target - eye;
    look.angle = d.y.atan2(d.x);
    look.pitch = d.z.atan2(d.truncate().length());
}

#[test]
fn pistol_kills_grunt_headless() {
    let mut map = combat_room();
    // Across the room on the 0.5 m step, in line with the start (y = 1.5).
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(10.0, 1.5),
        angle: std::f32::consts::PI,
        asleep: true,
    });
    let mut app = app(map);
    input(&mut app).fire = true;
    let mut killed = false;
    for _ in 0..240 {
        aim_at_actor(&mut app, 0);
        ticks(&mut app, 1);
        let fx = take_fx(&mut app);
        killed |= fx
            .combat
            .contains(&rr_core::combat::CombatEvent::ActorKilled(0));
        if killed {
            break;
        }
    }
    assert!(killed, "the grunt died");
    assert!(!app.world().resource::<LevelCombat>().0.actors[0].alive());
}

#[test]
fn shooting_glass_breaks_it_and_dirties_both_sectors() {
    // The start faces the pane, level, at eye height.
    let mut app = app(glass_rooms());
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    let fx = take_fx(&mut app);
    assert!(
        fx.combat
            .iter()
            .any(|e| matches!(e, rr_core::combat::CombatEvent::GlassBroken { .. })),
        "{:?}",
        fx.combat
    );
    let map = &app.world().resource::<CurrentMap>().0;
    assert!(map.walls.iter().all(|w| !w.glass));
    let dirty = &app.world().resource::<DirtySectors>().0;
    assert!(dirty.contains(&0) && dirty.contains(&1), "{dirty:?}");
}

#[test]
fn shooting_the_floor_reports_an_impact() {
    // Aiming down hits the floor in front of the player: pitch is up-positive.
    let mut app = app(combat_room());
    {
        let mut q = app.world_mut().query::<&mut Look>();
        q.single_mut(app.world_mut()).unwrap().pitch = -1.0;
    }
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    let fx = take_fx(&mut app);
    let point = fx
        .combat
        .iter()
        .find_map(|e| match e {
            rr_core::combat::CombatEvent::Impact { point, .. } => Some(*point),
            _ => None,
        })
        .expect("an impact");
    assert!(point.z.abs() < 1e-3, "hit the floor: {point}");
    assert!(
        point.x > 1.5,
        "in front of the player (facing east): {point}"
    );
}

#[test]
fn actor_bodies_ride_lifts_in_game() {
    // A sleeping grunt on the lift, facing away from the player in room A.
    let mut app = app(lift_shaft(
        "(kind: Lift(to: 2.0))",
        "actors: [(kind: Grunt, pos: (5.0, 2.0), angle_deg: 0.0)],",
    ));
    app.world_mut().resource_mut::<LevelMechanics>().0.toggle(0);
    // Mid-travel the grunt sits exactly on the lift plane after every tick: movers run after
    // combat, so this only holds if the carried bodies are written back to the actors.
    for _ in 0..20 {
        ticks(&mut app, 1);
        let lift = app.world().resource::<LevelMechanics>().0.movers[0].z;
        let z = app.world().resource::<LevelCombat>().0.actors[0].body.pos.z;
        assert!((z - lift).abs() < 1e-4, "grunt z {z} vs lift {lift}");
    }
    assert!(app.world().resource::<LevelMechanics>().0.movers[0].z > 0.0);
    ticks(&mut app, 100);
    assert!(matches!(
        app.world().resource::<LevelMechanics>().0.movers[0].motion,
        Motion::AtEnd { .. }
    ));
    let z = app.world().resource::<LevelCombat>().0.actors[0].body.pos.z;
    assert!((z - 2.0).abs() < 1e-4, "grunt rode the lift: z = {z}");
}

/// Walk east past the item at (3, 1.5) (start (2,2) faces east), then stop.
fn walk_past_item(app: &mut App) {
    input(app).forward = 1.0;
    ticks(app, 40);
    input(app).forward = 0.0;
    ticks(app, 30);
}

#[test]
fn dead_player_picks_nothing_up() {
    // 0.3 m from the start (2, 2): well inside pickup reach (radius 0.35 + 0.6).
    let mut app = app(door_rooms(
        "(kind: Door)",
        "items: [(kind: Key(Red), pos: (2.3, 2.0))],",
    ));
    assert!(!app.world().resource::<LevelMechanics>().0.taken[0]);
    let loadout = arsenal(&mut app).0.clone();
    health(&mut app).0.damage(1000);
    // `pickup_items` still runs on this tick (still `Playing`); `check_player_death` follows it.
    ticks(&mut app, 1);
    assert!(!app.world().resource::<LevelMechanics>().0.taken[0]);
    let mut q = app.world_mut().query::<&Inventory>();
    assert!(
        !q.single(app.world())
            .unwrap()
            .keys
            .contains(rr_core::map::Key::Red)
    );
    assert_eq!(arsenal(&mut app).0, loadout);
    assert_eq!(app.world().resource::<HudMessage>().text, "");
}

#[test]
fn ammo_pickup_refused_when_full_leaves_item() {
    let mut app = app(door_rooms(
        "(kind: Door)",
        "items: [(kind: PistolAmmo, pos: (3.0, 1.5))],",
    ));
    arsenal(&mut app).0.reserve[0] = 200;
    walk_past_item(&mut app);
    assert!(!app.world().resource::<LevelMechanics>().0.taken[0]);
    assert_eq!(arsenal(&mut app).0.reserve[0], 200);
    assert_eq!(app.world().resource::<HudMessage>().text, "");
    // Still standing in reach: once there is room, the next tick takes it.
    arsenal(&mut app).0.reserve[0] = 10;
    ticks(&mut app, 1);
    assert!(app.world().resource::<LevelMechanics>().0.taken[0]);
    assert_eq!(arsenal(&mut app).0.reserve[0], 22);
    assert_eq!(app.world().resource::<HudMessage>().text, "Pistol ammo");
}

#[test]
fn health_pack_heals_a_hurt_player() {
    let mut app = app(door_rooms(
        "(kind: Door)",
        "items: [(kind: HealthSmall, pos: (3.0, 1.5))],",
    ));
    health(&mut app).0.damage(30);
    walk_past_item(&mut app);
    assert!(app.world().resource::<LevelMechanics>().0.taken[0]);
    assert_eq!(health(&mut app).0.health.hp, 80);
}

#[test]
fn pipe_bombs_throw_and_detonate_in_game() {
    use rr_core::combat::CombatEvent;
    use rr_core::defs::AmmoKind;
    let mut app = app(combat_room());
    {
        let mut a = arsenal(&mut app);
        a.0.owned[WeaponId::PipeBombs.index()] = true;
        a.0.reserve[AmmoKind::Bombs.index()] = 1;
        a.0.current = WeaponId::PipeBombs;
    }
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    let combat = &app.world().resource::<LevelCombat>().0;
    assert_eq!(combat.projectiles.len(), 1, "the bomb is in the world");
    assert_eq!(combat.live_bombs(), 1);
    // The last bomb is out: the launcher stays up so it can be set off.
    ticks(&mut app, 90);
    assert_eq!(arsenal(&mut app).0.live_bombs, 1);
    assert_eq!(arsenal(&mut app).0.current, WeaponId::PipeBombs);
    take_fx(&mut app);
    input(&mut app).fire_pressed = true;
    ticks(&mut app, 1);
    let fx = take_fx(&mut app);
    assert!(fx.weapon.contains(&WeaponEvent::Detonate));
    assert!(fx.combat.contains(&CombatEvent::BombsDetonated));
    assert_eq!(
        fx.combat
            .iter()
            .filter(|e| matches!(e, CombatEvent::Explosion { .. }))
            .count(),
        1
    );
    assert!(
        app.world()
            .resource::<LevelCombat>()
            .0
            .projectiles
            .is_empty()
    );
}
