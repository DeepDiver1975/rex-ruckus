//! Episode progression: carried loadout, level stats and the episode end (headless).

use bevy::prelude::*;
use rr_core::carry::CarryOver;
use rr_core::defs::{Defs, WeaponId};
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::{combat_room, door_rooms};
use rr_core::map::{ActorKind, ActorSpawn, Map};
use rr_core::players::PlayerId;
use rr_core::vitals::Vitals;
use rr_core::weapons::Arsenal;
use rr_game::combat::{
    CombatSimPlugin, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerVitals, clear_fx,
    insert_defs,
};
use rr_game::coop::{CoopPlugin, EntryLoadout};
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin, Stats, start_episode};
use rr_game::flow::{FlowPlugin, PlayState, StateAge};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::player::{
    DevPartners, EYE_BELOW_TOP, LocalPlayer, Look, PendingInput, Player, PlayerBody,
    PlayerSimPlugin,
};

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
        CoopPlugin,
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
        CoopPlugin,
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

/// An episode on two maps with one dev partner (`PlayerId(1)`), started and on level 0.
fn two_player_episode() -> App {
    let mut app = episode_app(vec![combat_room(), door_rooms_level()]);
    app.insert_resource(DevPartners(1));
    start_episode(app.world_mut(), Difficulty::Normal);
    app
}

fn player_entity(app: &mut App, id: u8) -> Entity {
    let mut q = app.world_mut().query::<(Entity, &Player)>();
    q.iter(app.world())
        .find(|(_, p)| p.id == PlayerId(id))
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no player {id}"))
}

/// Edits player `id`'s vitals and arsenal.
fn give_to(app: &mut App, id: u8, f: impl FnOnce(&mut PlayerVitals, &mut PlayerArsenal, &Defs)) {
    let defs = app.world().resource::<GameDefs>().0.clone();
    let e = player_entity(app, id);
    let mut entity = app.world_mut().entity_mut(e);
    let mut v = PlayerVitals(entity.get::<PlayerVitals>().unwrap().0);
    let mut a = PlayerArsenal(entity.get::<PlayerArsenal>().unwrap().0.clone());
    f(&mut v, &mut a, &defs);
    entity.insert((v, a));
}

fn loadout_of(app: &mut App, id: u8) -> (Vitals, Arsenal) {
    let e = player_entity(app, id);
    let w = app.world();
    (
        w.get::<PlayerVitals>(e).unwrap().0,
        w.get::<PlayerArsenal>(e).unwrap().0.clone(),
    )
}

/// The local player (0) presses use on the Complete screen.
fn complete_level_two_players(app: &mut App) {
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Complete;
    ticks(app, 1);
    app.world_mut().resource_mut::<StateAge>().0 = 5.0;
    let mut q = app
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    q.single_mut(app.world_mut()).unwrap().use_pressed = true;
    ticks(app, 2);
}

#[test]
fn every_player_carries_own_loadout() {
    let mut app = two_player_episode();
    // Dev partner 1 starts with the chaingun, so it is player 0 that gets it and
    // player 1 swaps it for the shotgun.
    give_to(&mut app, 0, |_, a, defs| {
        a.0.give_weapon(defs, WeaponId::Chaingun);
    });
    give_to(&mut app, 1, |v, a, defs| {
        v.0.health.hp = 37;
        a.0 = Arsenal::new(defs);
        a.0.give_weapon(defs, WeaponId::Shotgun);
    });
    complete_level_two_players(&mut app);
    assert_eq!(app.world().resource::<Episode>().index, 1);
    let (v0, a0) = loadout_of(&mut app, 0);
    assert_eq!(v0.health.hp, 100);
    assert!(a0.owned[WeaponId::Chaingun.index()]);
    assert!(!a0.owned[WeaponId::Shotgun.index()]);
    let (v1, a1) = loadout_of(&mut app, 1);
    assert_eq!(v1.health.hp, 37);
    assert!(a1.owned[WeaponId::Shotgun.index()]);
    assert!(!a1.owned[WeaponId::Chaingun.index()]);
    for (id, v, a) in [(0, v0, a0), (1, v1, a1)] {
        let e = player_entity(&mut app, id);
        let inv = app
            .world()
            .get::<rr_game::combat::PlayerInventory>(e)
            .unwrap()
            .0;
        assert_eq!(
            app.world().get::<EntryLoadout>(e).unwrap().0,
            CarryOver::take(&v, &a, &inv)
        );
    }
}

#[test]
fn downed_player_carries_entry_loadout() {
    let mut app = two_player_episode();
    // Player 1 entered level 0 with the default loadout.
    let defs = app.world().resource::<GameDefs>().0.clone();
    let default = CarryOver::take(&Vitals::new(), &Arsenal::new(&defs), &Default::default());
    let e1 = player_entity(&mut app, 1);
    app.world_mut().entity_mut(e1).insert(EntryLoadout(default));
    give_to(&mut app, 1, |v, a, defs| {
        a.0.give_weapon(defs, WeaponId::Shotgun);
        v.0.health.hp = 0;
    });
    complete_level_two_players(&mut app);
    let (v1, a1) = loadout_of(&mut app, 1);
    assert_eq!(v1.health.hp, 100);
    assert!(!a1.owned[WeaponId::Shotgun.index()]);
}

#[test]
fn single_player_carry_is_unchanged() {
    let app = on_level_two();
    let ep = app.world().resource::<Episode>();
    let c = ep.entries[0].as_ref().expect("player 0 carried");
    assert_eq!(c.vitals.armour, 50);
    assert!(c.arsenal.owned[WeaponId::Shotgun.index()]);
    assert!(ep.entries[1..].iter().all(Option::is_none));
}
