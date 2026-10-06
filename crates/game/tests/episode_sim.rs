//! Episode progression: carried loadout, level stats and the episode end (headless).

use bevy::prelude::*;
use rr_core::defs::{Defs, WeaponId};
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, door_rooms};
use rr_core::map::{ActorKind, ActorSpawn, Map};
use rr_core::vitals::Vitals;
use rr_core::weapons::Arsenal;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerVitals, clear_fx,
    insert_defs,
};
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin, Stats, start_episode};
use rr_game::flow::{FlowPlugin, PlayState, StateAge};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{EYE_BELOW_TOP, Look, PendingInput, PlayerBody, PlayerSimPlugin};

/// A headless sim running an episode over the given fixture maps.
fn episode_app(maps: Vec<Map>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, maps[0].clone(), Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    let mut episode = Episode::new(EpisodeDef {
        name: "t".into(),
        levels: vec![],
    });
    episode.maps = Some(maps);
    app.insert_resource(episode);
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        EpisodePlugin,
    ));
    app.update();
    app
}

fn sim_app(map: Map) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, map, Difficulty::Normal);
    insert_defs(&mut app, Defs::builtin());
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        EpisodePlugin,
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

/// Edits the player's vitals and arsenal.
fn give(app: &mut App, f: impl FnOnce(&mut PlayerVitals, &mut PlayerArsenal, &Defs)) {
    let defs = app.world().resource::<GameDefs>().0.clone();
    let mut q = app
        .world_mut()
        .query::<(&mut PlayerVitals, &mut PlayerArsenal)>();
    let (mut v, mut a) = q.single_mut(app.world_mut()).unwrap();
    f(&mut v, &mut a, &defs);
}

fn player_vitals_arsenal(app: &mut App) -> (Vitals, Arsenal) {
    let mut q = app.world_mut().query::<(&PlayerVitals, &PlayerArsenal)>();
    let (v, a) = q.single(app.world()).unwrap();
    (v.0, a.0.clone())
}

fn door_rooms_level() -> Map {
    door_rooms("(kind: Door)", "switches: [(wall: (7, 0), action: Exit)],")
}

fn grunt(x: f32, y: f32) -> ActorSpawn {
    ActorSpawn {
        kind: ActorKind::Grunt,
        pos: Vec2::new(x, y),
        angle: std::f32::consts::PI,
        asleep: true,
        skill: Difficulty::Easy,
        on_death: None,
    }
}

/// combat_room plus one awake grunt in the main room.
fn arena() -> Map {
    let mut map = combat_room();
    map.actors.push(ActorSpawn {
        asleep: false,
        ..grunt(7.0, 1.5)
    });
    map
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

/// Shoots until the actor is dead (one tick at a time, so no frame boundary is crossed).
fn kill_actor(app: &mut App, actor: usize) {
    input(app).fire = true;
    for _ in 0..600 {
        if !app.world().resource::<LevelCombat>().0.actors[actor].alive() {
            break;
        }
        aim_at_actor(app, actor);
        ticks(app, 1);
    }
    input(app).fire = false;
    assert!(!app.world().resource::<LevelCombat>().0.actors[actor].alive());
}

/// Puts the game in `state`, lets the screen's grace period pass and sends a `press`.
fn leave_state(app: &mut App, state: PlayState, press: impl FnOnce(&mut PendingInput)) {
    *app.world_mut().resource_mut::<PlayState>() = state;
    // The age resets on the tick that sees the new state.
    ticks(app, 1);
    app.world_mut().resource_mut::<StateAge>().0 = 5.0;
    press(&mut input(app));
    ticks(app, 2);
}

fn complete_level(app: &mut App) {
    leave_state(app, PlayState::Complete, |i| i.use_pressed = true);
}

/// Level 2 reached with 50 armour and the shotgun.
fn on_level_two() -> App {
    let mut app = episode_app(vec![combat_room(), door_rooms_level()]);
    start_episode(app.world_mut(), Difficulty::Normal);
    give(&mut app, |v, a, defs| {
        v.0.armour = 50;
        a.0.give_weapon(defs, WeaponId::Shotgun);
    });
    complete_level(&mut app);
    app
}

#[test]
fn completing_a_level_loads_the_next_with_the_loadout() {
    let mut app = on_level_two();
    assert_eq!(app.world().resource::<Episode>().index, 1);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
    let (v, a) = player_vitals_arsenal(&mut app);
    assert_eq!(v.armour, 50);
    assert!(a.owned[WeaponId::Shotgun.index()]);
}

#[test]
fn death_restarts_with_the_entry_loadout() {
    let mut app = on_level_two();
    give(&mut app, |v, a, defs| {
        v.0.armour = 0;
        a.0.give_weapon(defs, WeaponId::Chaingun);
    });
    leave_state(&mut app, PlayState::Dead, |i| i.fire_pressed = true);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
    let (v, a) = player_vitals_arsenal(&mut app);
    assert_eq!(v.armour, 50);
    assert!(a.owned[WeaponId::Shotgun.index()]);
    assert!(!a.owned[WeaponId::Chaingun.index()]);
}

#[test]
fn finishing_the_last_level_ends_the_episode_with_totals() {
    let mut app = episode_app(vec![combat_room()]);
    start_episode(app.world_mut(), Difficulty::Normal);
    ticks(&mut app, 60);
    complete_level(&mut app);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::EpisodeEnd);
    assert!(app.world().resource::<Episode>().totals.ticks >= 60);
}

#[test]
fn direct_level_runs_still_restart_on_complete() {
    let mut app = sim_app(combat_room()); // no Episode resource
    complete_level(&mut app);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Playing);
}

#[test]
fn kills_are_counted_while_playing() {
    let mut app = episode_app(vec![arena()]);
    start_episode(app.world_mut(), Difficulty::Normal);
    let total = app.world().resource::<Stats>().0.kills_total;
    assert!(total > 0);
    // Many fixed ticks pass in one frame (no `clear_fx` between them): one kill counts once.
    kill_actor(&mut app, 0);
    ticks(&mut app, 3);
    assert_eq!(app.world().resource::<Stats>().0.kills, 1);
}

#[test]
fn kills_are_counted_once_across_frames() {
    let mut map = combat_room();
    map.actors.push(grunt(10.0, 1.5));
    map.actors.push(grunt(10.0, 1.5));
    let mut app = episode_app(vec![map]);
    start_episode(app.world_mut(), Difficulty::Normal);
    kill_actor(&mut app, 0);
    ticks(&mut app, 2);
    assert_eq!(app.world().resource::<Stats>().0.kills, 1);
    // The frame ends: the queue empties. The next frame's events must all be counted.
    app.world_mut().run_system_cached(clear_fx).unwrap();
    assert!(app.world().resource::<FxQueue>().combat.is_empty());
    kill_actor(&mut app, 1);
    ticks(&mut app, 2);
    assert_eq!(app.world().resource::<Stats>().0.kills, 2);
}

#[test]
fn stats_restart_with_each_level_and_feed_the_totals() {
    let mut app = episode_app(vec![combat_room(), door_rooms_level()]);
    start_episode(app.world_mut(), Difficulty::Normal);
    ticks(&mut app, 60);
    complete_level(&mut app);
    // Only the tick after the level loaded is on the new level's counter.
    assert!(app.world().resource::<Stats>().0.ticks <= 2);
    assert!(app.world().resource::<Episode>().totals.ticks >= 60);
}
