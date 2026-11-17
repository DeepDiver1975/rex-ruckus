//! `NetMode` and the sim gates: a client never simulates, a host keeps running under the pause
//! menu, and the level bookkeeping (`LevelRuns`, `LocalSeat`).

use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::combat_room;
use rr_core::players::PlayerId;
use rr_game::combat::{CombatSimPlugin, LevelCombat, insert_defs};
use rr_game::flow::{
    FlowPlugin, LevelRuns, LocalSeat, PausedFrom, PlayState, RESTART_DELAY, StateAge, restart_level,
};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::net::NetMode;
use rr_game::player::{LocalPlayer, PendingInput, Player, PlayerBody, PlayerSimPlugin};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    insert_level(&mut app, combat_room(), Difficulty::Normal);
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

fn walk_forward(app: &mut App) {
    let mut q = app
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    for mut input in q.iter_mut(app.world_mut()) {
        input.forward = 1.0;
    }
}

fn body_pos(app: &mut App) -> Vec3 {
    let mut q = app
        .world_mut()
        .query_filtered::<&PlayerBody, With<LocalPlayer>>();
    q.single(app.world()).unwrap().0.pos
}

fn actor_positions(app: &App) -> Vec<Vec3> {
    app.world()
        .resource::<LevelCombat>()
        .0
        .actors
        .iter()
        .map(|a| a.body.pos)
        .collect()
}

fn projectile_positions(app: &App) -> Vec<Vec3> {
    app.world()
        .resource::<LevelCombat>()
        .0
        .projectiles
        .iter()
        .map(|p| p.pos)
        .collect()
}

#[test]
fn client_mode_never_simulates() {
    let mut app = app();
    app.insert_resource(NetMode::Client);
    walk_forward(&mut app);
    let (body, actors, shots) = (
        body_pos(&mut app),
        actor_positions(&app),
        projectile_positions(&app),
    );
    ticks(&mut app, 60);
    assert_eq!(body_pos(&mut app), body);
    assert_eq!(actor_positions(&app), actors);
    assert_eq!(projectile_positions(&app), shots);
}

#[test]
fn single_mode_pauses_as_before() {
    let mut app = app();
    app.insert_resource(PlayState::Paused);
    walk_forward(&mut app);
    let body = body_pos(&mut app);
    ticks(&mut app, 60);
    assert_eq!(body_pos(&mut app), body);
}

#[test]
fn host_keeps_running_under_the_pause_menu() {
    let mut app = app();
    app.insert_resource(NetMode::Host);
    app.insert_resource(PlayState::Paused);
    app.insert_resource(PausedFrom(PlayState::Playing));
    walk_forward(&mut app);
    let body = body_pos(&mut app);
    ticks(&mut app, 60);
    assert_ne!(body_pos(&mut app), body);
}

#[test]
fn host_paused_from_dead_stays_frozen() {
    let mut app = app();
    app.insert_resource(NetMode::Host);
    app.insert_resource(PlayState::Paused);
    app.insert_resource(PausedFrom(PlayState::Dead));
    walk_forward(&mut app);
    let body = body_pos(&mut app);
    ticks(&mut app, 60);
    assert_eq!(body_pos(&mut app), body);
}

#[test]
fn restart_counts_runs() {
    let mut app = app();
    restart_level(app.world_mut());
    restart_level(app.world_mut());
    assert_eq!(app.world().resource::<LevelRuns>().0, 2);
}

#[test]
fn local_seat_names_the_local_player() {
    let mut app = app();
    app.insert_resource(LocalSeat(PlayerId(2)));
    restart_level(app.world_mut());
    let mut q = app
        .world_mut()
        .query_filtered::<&Player, With<LocalPlayer>>();
    assert_eq!(q.single(app.world()).unwrap().id, PlayerId(2));
}

#[test]
fn client_never_restarts_from_the_death_screen() {
    let mut app = app();
    app.insert_resource(NetMode::Client);
    app.insert_resource(PlayState::Dead);
    // The first tick sees the state change and zeroes the age; age it after that.
    ticks(&mut app, 1);
    app.insert_resource(StateAge(RESTART_DELAY + 1.0));
    let mut q = app
        .world_mut()
        .query_filtered::<&mut PendingInput, With<LocalPlayer>>();
    for mut input in q.iter_mut(app.world_mut()) {
        input.fire_pressed = true;
    }
    ticks(&mut app, 5);
    assert_eq!(app.world().resource::<LevelRuns>().0, 0);
    assert_eq!(*app.world().resource::<PlayState>(), PlayState::Dead);
}
