//! The menus drive the game flow (headless: actions are injected through `MenuInput`).

use bevy::prelude::*;
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::combat_room;
use rr_core::map::Map;
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin};
use rr_game::flow::{FlowPlugin, LevelDifficulty, PlayState};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::menu::{MenuAction, MenuInput, MenuPlugin, MenuRoot, MenuScreen, Screen};
use rr_game::player::PlayerSimPlugin;

/// An episode over the given maps, started at the title screen.
fn menu_app(maps: Vec<Map>) -> App {
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
    app.insert_resource(PlayState::Menu);
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        EpisodePlugin,
        MenuPlugin { enabled: true },
    ));
    app.update();
    app
}

fn press(app: &mut App, action: MenuAction) {
    app.world_mut().resource_mut::<MenuInput>().activate = Some(action);
    app.update();
}

fn screen(app: &App) -> Option<Screen> {
    app.world().resource::<MenuScreen>().0
}

fn state(app: &App) -> PlayState {
    *app.world().resource::<PlayState>()
}

fn roots(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<(), With<MenuRoot>>()
        .iter(app.world())
        .count()
}

#[test]
fn starts_on_the_main_menu() {
    let mut app = menu_app(vec![combat_room()]);
    assert_eq!(state(&app), PlayState::Menu);
    assert_eq!(screen(&app), Some(Screen::Main));
    assert_eq!(roots(&mut app), 1);
}

#[test]
fn new_game_on_hard_starts_playing_level_one() {
    let mut app = menu_app(vec![combat_room(), combat_room()]);
    press(&mut app, MenuAction::NewGame);
    assert_eq!(screen(&app), Some(Screen::Difficulty));
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Hard));
    assert_eq!(state(&app), PlayState::Playing);
    assert_eq!(
        app.world().resource::<LevelDifficulty>().0,
        Difficulty::Hard
    );
    assert_eq!(app.world().resource::<Episode>().index, 0);
    app.update();
    assert_eq!(screen(&app), None);
    assert_eq!(roots(&mut app), 0);
}

#[test]
fn difficulty_back_returns_to_main() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::Back);
    assert_eq!(screen(&app), Some(Screen::Main));
    assert_eq!(state(&app), PlayState::Menu);
}

#[test]
fn pause_menu_quit_to_menu_and_resume() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.update();
    assert_eq!(screen(&app), Some(Screen::Pause));
    press(&mut app, MenuAction::Resume);
    assert_eq!(state(&app), PlayState::Playing);
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    press(&mut app, MenuAction::QuitToMenu);
    assert_eq!(state(&app), PlayState::Menu);
    app.update();
    assert_eq!(screen(&app), Some(Screen::Main));
}

#[test]
fn options_from_pause_go_back_to_pause() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.update();
    press(&mut app, MenuAction::Options);
    assert_eq!(screen(&app), Some(Screen::Options));
    press(&mut app, MenuAction::Back);
    assert_eq!(screen(&app), Some(Screen::Pause));
    assert_eq!(state(&app), PlayState::Paused);
}

#[test]
fn quit_game_exits() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::QuitGame);
    assert!(app.should_exit().is_some());
}

#[test]
fn restart_from_pause_resumes_play() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.update();
    press(&mut app, MenuAction::RestartLevel);
    assert_eq!(state(&app), PlayState::Playing);
    app.world_mut().run_schedule(FixedUpdate);
    assert!(!app.world().resource::<rr_game::flow::RestartRequested>().0);
    app.update();
    assert_eq!(screen(&app), None);
}
