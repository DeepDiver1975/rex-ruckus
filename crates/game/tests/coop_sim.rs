//! Co-op in the headless sim: several `Player { id }` entities, one of them the `LocalPlayer`.

use bevy::ecs::system::{RunSystemOnce, SystemId};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use rr_core::actors::Target;
use rr_core::audio::QuipTrigger;
use rr_core::collide::Body;
use rr_core::combat::CombatEvent;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, engine_room};
use rr_core::map::{ActorKind, ActorSpawn, Item, ItemKind, Key, Map};
use rr_core::players::PlayerId;
use rr_core::projectile::Shooter;
use rr_core::weapons::WeaponEvent;
use rr_game::audio::combat_quips;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerInventory, PlayerVitals, clear_fx,
    insert_defs,
};
use rr_game::coords::{core_angle_to_yaw, to_bevy};
use rr_game::flow::{FlowPlugin, PlayState};
use rr_game::hud::{DamageFlash, FlashColor, read_hurt};
use rr_game::level::CurrentMap;
use rr_game::mechanics::{HudMessage, LevelMechanics, MechanicsSimPlugin, insert_level};
use rr_game::models::{ModelSlot, ModelsPlugin, Rig};
use rr_game::partners::{PartnerVisual, PartnersPlugin};
use rr_game::player::{
    Inventory, LocalPlayer, Look, LookSettings, PLAYER_RADIUS, PendingInput, Player, PlayerBody,
    PlayerSimPlugin, PlayerTuning, PrevFeet, read_input, spawn_coop_player,
};
use rr_game::viewmodel::{ViewState, read_fx};

fn app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map, Difficulty::Normal);
    let mut defs = Defs::builtin();
    for e in &mut defs.enemies {
        e.aim_time = 0.0;
    }
    insert_defs(&mut app, defs);
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        PartnersPlugin,
    ));
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.world_mut().run_schedule(FixedUpdate);
    }
}

fn spawn_coop(app: &mut App, id: PlayerId) -> Entity {
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  map: Res<CurrentMap>,
                  tuning: Res<PlayerTuning>,
                  defs: Res<GameDefs>| {
                spawn_coop_player(&mut commands, &map.0, &tuning.0, &defs.0, id)
            },
        )
        .unwrap()
}

/// The entity of player `id`.
fn player(app: &mut App, id: PlayerId) -> Entity {
    let mut q = app.world_mut().query::<(Entity, &Player)>();
    q.iter(app.world())
        .find(|(_, p)| p.id == id)
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no player {id:?}"))
}

fn body(app: &mut App, id: PlayerId) -> Body {
    let e = player(app, id);
    app.world().get::<PlayerBody>(e).unwrap().0
}

fn health(app: &mut App, id: PlayerId) -> i32 {
    let e = player(app, id);
    app.world().get::<PlayerVitals>(e).unwrap().0.health.hp
}

fn place(app: &mut App, id: PlayerId, x: f32, y: f32) {
    let e = player(app, id);
    let map = app.world().resource::<CurrentMap>().0.clone();
    let mut b = app.world().get::<PlayerBody>(e).unwrap().0;
    let spawned = Body::spawn(&map, Vec2::new(x, y), b.radius, b.height).unwrap();
    b.pos = spawned.pos;
    b.sector = spawned.sector;
    b.vel = Vec3::ZERO;
    app.world_mut().get_mut::<PlayerBody>(e).unwrap().0 = b;
    app.world_mut().get_mut::<PrevFeet>(e).unwrap().0 = b.pos;
}

fn kill(app: &mut App, id: PlayerId) {
    let e = player(app, id);
    app.world_mut()
        .get_mut::<PlayerVitals>(e)
        .unwrap()
        .0
        .damage(1000);
}

#[test]
fn two_players_spawn_apart() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    let (a, b) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    assert!(
        a.pos.truncate().distance(b.pos.truncate()) >= 2.0 * PLAYER_RADIUS,
        "{a:?} {b:?}"
    );
    // The co-op player is no local one and carries the default loadout.
    let e = player(&mut app, PlayerId(1));
    assert!(app.world().get::<LocalPlayer>(e).is_none());
    assert_eq!(health(&mut app, PlayerId(1)), 100);
    let mut q = app
        .world_mut()
        .query_filtered::<&Player, With<LocalPlayer>>();
    let locals: Vec<Player> = q.iter(app.world()).copied().collect();
    assert_eq!(locals, [Player { id: PlayerId(0) }]);
}

/// The sim plus the real input system, with a grabbed cursor and no window.
fn input_rig(map: Map) -> (App, SystemId) {
    let mut app = app(map);
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<rr_game::bindings::Bindings>()
        .insert_resource(LookSettings::default());
    app.world_mut().spawn(CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        ..default()
    });
    let read = app.world_mut().register_system(read_input);
    (app, read)
}

#[test]
fn local_input_moves_only_local_player() {
    let (mut app, read) = input_rig(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    let (a0, b0) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    for _ in 0..10 {
        app.world_mut().run_system(read).unwrap();
        ticks(&mut app, 1);
    }
    let (a1, b1) = (body(&mut app, PlayerId(0)), body(&mut app, PlayerId(1)));
    assert!(
        a1.pos.x > a0.pos.x + 0.1,
        "local player moved: {a0:?} -> {a1:?}"
    );
    assert_eq!(b1.pos.truncate(), b0.pos.truncate(), "player 1 stayed");

    // Whoever drives player 1 writes its input directly.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    let e = player(&mut app, PlayerId(1));
    app.world_mut().get_mut::<PendingInput>(e).unwrap().forward = 1.0;
    for _ in 0..10 {
        app.world_mut().run_system(read).unwrap();
        ticks(&mut app, 1);
    }
    let b2 = body(&mut app, PlayerId(1));
    assert!(
        b2.pos.x > b1.pos.x + 0.1,
        "player 1 moved: {b1:?} -> {b2:?}"
    );
}

#[test]
fn enemy_targets_nearer_player() {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(7.0, 7.0),
        angle: std::f32::consts::PI,
        asleep: false,
        skill: Difficulty::Easy,
        on_death: None,
    });
    let mut app = app(map);
    spawn_coop(&mut app, PlayerId(1));
    place(&mut app, PlayerId(1), 5.5, 7.0);
    let mut target = None;
    for _ in 0..30 {
        ticks(&mut app, 1);
        target = Some(app.world().resource::<LevelCombat>().0.actors[0].target);
        if target == Some(Target::Player(PlayerId(1))) {
            break;
        }
    }
    assert_eq!(target, Some(Target::Player(PlayerId(1))));
}

#[test]
fn sim_orders_players_by_id() {
    let mut app = app(engine_room(""));
    // A fresh set of players spawned in reverse order: id 1, then id 0.
    let old = player(&mut app, PlayerId(0));
    app.world_mut().despawn(old);
    spawn_coop(&mut app, PlayerId(1));
    spawn_coop(&mut app, PlayerId(0));
    // Player 1 stands in the slime pit, player 0 in the hall.
    place(&mut app, PlayerId(1), 11.0, 2.0);
    place(&mut app, PlayerId(0), 1.0, 2.0);
    let mut hurt = Vec::new();
    for _ in 0..120 {
        ticks(&mut app, 1);
        let mut fx = app.world_mut().resource_mut::<FxQueue>();
        hurt.extend(fx.combat.drain(..).filter_map(|e| match e {
            CombatEvent::PlayerHurt { player, .. } => Some(player),
            _ => None,
        }));
        if !hurt.is_empty() {
            break;
        }
    }
    assert_eq!(hurt, [PlayerId(1)], "the slime burned player 1");
    assert!(health(&mut app, PlayerId(1)) < 100);
    assert_eq!(health(&mut app, PlayerId(0)), 100);
}

#[test]
fn level_continues_while_a_partner_lives() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    kill(&mut app, PlayerId(0));
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
    // The corpse ignores its input.
    let before = body(&mut app, PlayerId(0));
    let e = player(&mut app, PlayerId(0));
    app.world_mut().get_mut::<PendingInput>(e).unwrap().forward = 1.0;
    ticks(&mut app, 10);
    let after = body(&mut app, PlayerId(0));
    assert_eq!(
        after.pos.truncate(),
        before.pos.truncate(),
        "the dead stay put"
    );

    kill(&mut app, PlayerId(1));
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
}

fn input(app: &mut App, id: PlayerId) -> Mut<'_, PendingInput> {
    let e = player(app, id);
    app.world_mut().get_mut::<PendingInput>(e).unwrap()
}

fn carried(app: &mut App, id: PlayerId) -> Mut<'_, PlayerInventory> {
    let e = player(app, id);
    app.world_mut().get_mut::<PlayerInventory>(e).unwrap()
}

fn message(app: &App) -> String {
    app.world().resource::<HudMessage>().text.clone()
}

fn hurt(player: PlayerId) -> CombatEvent {
    CombatEvent::PlayerHurt {
        player,
        amount: 20,
        from: Vec3::ZERO,
    }
}

/// The HUD's damage flash after this frame's events `events`.
fn flash_after(app: &mut App, events: Vec<CombatEvent>) -> f32 {
    app.world_mut().resource_mut::<DamageFlash>().0 = 0.0;
    app.world_mut().resource_mut::<FxQueue>().combat = events;
    app.world_mut().run_system_once(read_hurt).unwrap();
    app.world_mut().run_system_once(clear_fx).unwrap();
    app.world().resource::<DamageFlash>().0
}

#[test]
fn remote_hurt_does_not_flash_hud() {
    let mut app = app(combat_room());
    app.init_resource::<DamageFlash>()
        .insert_resource(FlashColor(Color::WHITE));
    spawn_coop(&mut app, PlayerId(1));
    assert_eq!(flash_after(&mut app, vec![hurt(PlayerId(1))]), 0.0);
    assert!(flash_after(&mut app, vec![hurt(PlayerId(0))]) > 0.0);
}

/// Fires player `id`'s gun for one tick and returns the viewmodel state the frame then shows.
fn view_after_fire(app: &mut App, id: PlayerId) -> ViewState {
    *app.world_mut().resource_mut::<ViewState>() = ViewState::default();
    input(app, id).fire_pressed = true;
    ticks(app, 1);
    let fired = app.world().resource::<FxQueue>().weapon.clone();
    assert!(
        fired
            .iter()
            .any(|(p, e)| *p == id && matches!(e, WeaponEvent::Fire { .. })),
        "player {id:?} fired: {fired:?}"
    );
    app.world_mut().run_system_once(read_fx).unwrap();
    app.world_mut().run_system_once(clear_fx).unwrap();
    std::mem::take(&mut *app.world_mut().resource_mut::<ViewState>())
}

#[test]
fn remote_fire_does_not_kick_viewmodel() {
    let mut app = app(combat_room());
    app.init_resource::<ViewState>();
    spawn_coop(&mut app, PlayerId(1));
    // Let the starting weapon come up.
    ticks(&mut app, 60);
    app.world_mut().run_system_once(clear_fx).unwrap();
    assert_eq!(view_after_fire(&mut app, PlayerId(1)), ViewState::default());
    assert!(view_after_fire(&mut app, PlayerId(0)).recoil.z > 0.0);
}

#[test]
fn remote_pickup_shows_no_message() {
    let mut map = combat_room();
    map.items.push(Item {
        kind: ItemKind::Key(Key::Red),
        pos: Vec2::new(6.0, 6.5),
        skill: Difficulty::Easy,
    });
    let mut app = app(map);
    spawn_coop(&mut app, PlayerId(1));
    place(&mut app, PlayerId(1), 6.0, 6.5);
    ticks(&mut app, 1);
    assert!(app.world().resource::<LevelMechanics>().0.taken[0]);
    let e = player(&mut app, PlayerId(1));
    assert!(
        app.world()
            .get::<Inventory>(e)
            .unwrap()
            .keys
            .contains(Key::Red)
    );
    assert_eq!(message(&app), "");
}

#[test]
fn remote_kill_gives_no_quip() {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(7.0, 7.0),
        angle: 0.0,
        asleep: true,
        skill: Difficulty::Easy,
        on_death: None,
    });
    let app = app(map);
    let combat = &app.world().resource::<LevelCombat>().0;
    let kill = |id| CombatEvent::ActorKilled {
        actor: 0,
        by: Shooter::Player(id),
    };
    assert_eq!(
        combat_quips(&[kill(PlayerId(1))], Some(combat), Some(PlayerId(0))),
        []
    );
    assert_eq!(
        combat_quips(&[kill(PlayerId(0))], Some(combat), Some(PlayerId(0))),
        [QuipTrigger::Kill(ActorKind::Grunt)]
    );
}

#[test]
fn partner_inventory_messages_stay_off_the_hud() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    carried(&mut app, PlayerId(1)).0.fuel = 0.01;
    input(&mut app, PlayerId(1)).toggle_jetpack = true;
    ticks(&mut app, 3);
    assert!(!carried(&mut app, PlayerId(1)).0.jetpack_on, "ran dry");
    assert_eq!(message(&app), "");
    // The local player's own run-dry still says so.
    carried(&mut app, PlayerId(0)).0.fuel = 0.01;
    input(&mut app, PlayerId(0)).toggle_jetpack = true;
    ticks(&mut app, 3);
    assert!(message(&app).contains("fuel"), "{}", message(&app));
}

#[test]
fn partner_use_messages_stay_off_the_hud() {
    // Both face east, at the locked door of door_rooms.
    let map = rr_core::fixtures::door_rooms("(kind: Door, lock: Some(Red))", "");
    let mut app = app(map);
    spawn_coop(&mut app, PlayerId(1));
    place(&mut app, PlayerId(1), 3.5, 1.0);
    input(&mut app, PlayerId(1)).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(message(&app), "");
    place(&mut app, PlayerId(0), 3.5, 3.0);
    input(&mut app, PlayerId(0)).use_pressed = true;
    ticks(&mut app, 1);
    assert!(message(&app).contains("red"), "{}", message(&app));
}

#[test]
fn only_the_local_player_restarts() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    kill(&mut app, PlayerId(0));
    kill(&mut app, PlayerId(1));
    ticks(&mut app, 70);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
    input(&mut app, PlayerId(1)).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
    input(&mut app, PlayerId(0)).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
}

#[test]
fn the_dead_drop_out_of_the_sky_and_toggle_nothing() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    carried(&mut app, PlayerId(1)).0.fuel = 100.0;
    input(&mut app, PlayerId(1)).toggle_jetpack = true;
    input(&mut app, PlayerId(1)).jump = true;
    ticks(&mut app, 30);
    let up = body(&mut app, PlayerId(1)).pos.z;
    assert!(up > 1.0, "player 1 flies: {up}");
    kill(&mut app, PlayerId(1));
    ticks(&mut app, 60);
    let down = body(&mut app, PlayerId(1));
    assert!(down.on_ground && down.pos.z < up, "the dead fall: {down:?}");

    // Dying switched the jetpack off; a dead player's toggles do nothing.
    assert!(!carried(&mut app, PlayerId(1)).0.jetpack_on, "jetpack off");
    let mut c = carried(&mut app, PlayerId(1));
    c.0.battery = 100.0;
    input(&mut app, PlayerId(1)).toggle_jetpack = true;
    input(&mut app, PlayerId(1)).toggle_nv = true;
    ticks(&mut app, 1);
    let c = carried(&mut app, PlayerId(1)).0;
    assert!(!c.jetpack_on && !c.nv_on, "{c:?}");
}

/// Every partner visual root and the player it shows.
fn partner_visuals(app: &mut App) -> Vec<(Entity, Entity)> {
    let mut q = app.world_mut().query::<(Entity, &PartnerVisual)>();
    q.iter(app.world()).map(|(e, v)| (e, v.player)).collect()
}

#[test]
fn partner_gets_a_visual_local_does_not() {
    let mut app = app(combat_room());
    app.update();
    assert!(partner_visuals(&mut app).is_empty(), "single player: none");
    spawn_coop(&mut app, PlayerId(1));
    app.update();
    app.update();
    let shown = partner_visuals(&mut app);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].1, player(&mut app, PlayerId(1)));
}

#[test]
fn partner_visual_follows_body() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    app.update();
    place(&mut app, PlayerId(1), 6.0, 2.5);
    let e = player(&mut app, PlayerId(1));
    app.world_mut().get_mut::<Look>(e).unwrap().angle = 1.0;
    app.update();
    let feet = body(&mut app, PlayerId(1)).pos;
    let (root, _) = partner_visuals(&mut app)[0];
    let t = *app.world().get::<Transform>(root).unwrap();
    assert!(
        t.translation.abs_diff_eq(to_bevy(feet), 1e-4),
        "{} vs {}",
        t.translation,
        to_bevy(feet)
    );
    let want = Quat::from_rotation_y(core_angle_to_yaw(1.0));
    assert!(t.rotation.abs_diff_eq(want, 1e-4), "{:?}", t.rotation);
}

#[test]
fn partner_visual_despawns_with_player() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    app.update();
    assert_eq!(partner_visuals(&mut app).len(), 1);
    let e = player(&mut app, PlayerId(1));
    app.world_mut().despawn(e);
    app.update();
    assert!(partner_visuals(&mut app).is_empty());
}

#[test]
fn restart_drops_partner_visuals() {
    let mut app = app(combat_room());
    spawn_coop(&mut app, PlayerId(1));
    app.update();
    assert_eq!(partner_visuals(&mut app).len(), 1);
    kill(&mut app, PlayerId(0));
    kill(&mut app, PlayerId(1));
    ticks(&mut app, 70);
    input(&mut app, PlayerId(0)).use_pressed = true;
    ticks(&mut app, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
    app.update();
    assert!(partner_visuals(&mut app).is_empty());
}

#[test]
fn partner_visual_holds_the_rex_model() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        // The asset types ModelsPlugin loads (no loaders: loads just fail).
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>();
    insert_level(&mut app, combat_room(), Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        ModelsPlugin,
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        PartnersPlugin,
    ));
    app.update();
    spawn_coop(&mut app, PlayerId(1));
    app.update();
    let (root, _) = partner_visuals(&mut app)[0];
    let mut q = app.world_mut().query::<(&ChildOf, &ModelSlot)>();
    let rigs: Vec<_> = q
        .iter(app.world())
        .filter(|(c, _)| c.parent() == root)
        .map(|(_, s)| s.rig)
        .collect();
    assert_eq!(rigs, [Some(Rig::Player)]);
}
