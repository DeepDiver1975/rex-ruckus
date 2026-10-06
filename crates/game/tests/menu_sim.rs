//! The menus drive the game flow (headless: actions are injected through `MenuInput`).

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::combat_room;
use rr_core::map::Map;
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin};
use rr_game::flow::{FlowPlugin, LevelDifficulty, PlayState};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::menu::{MenuAction, MenuInput, MenuPlugin, MenuRoot, MenuScreen, Screen};
use rr_game::player::{PendingInput, PlayerBody, PlayerSimPlugin};

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
fn restart_from_pause_rebuilds_the_level_at_once() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    let start = player_pos(&mut app);
    {
        let mut q = app.world_mut().query::<&mut PlayerBody>();
        q.single_mut(app.world_mut()).unwrap().0.pos.x += 3.0;
    }
    assert_ne!(player_pos(&mut app), start);
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.update();
    press(&mut app, MenuAction::RestartLevel);
    // No fixed tick has run: the restart happened in the action itself.
    assert_eq!(state(&app), PlayState::Playing);
    assert_eq!(player_pos(&mut app), start);
    assert!(!app.world().resource::<rr_game::flow::RestartRequested>().0);
}

fn player_pos(app: &mut App) -> Vec3 {
    let mut q = app.world_mut().query::<&PlayerBody>();
    q.single(app.world()).unwrap().0.pos
}

/// The click that chooses Resume must not also grab the cursor (or fire).
#[test]
fn menu_click_does_not_grab_the_cursor() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    app.init_resource::<ButtonInput<MouseButton>>();
    // A window brings its own `CursorOptions`.
    let cursor = app
        .world_mut()
        .spawn(Window {
            focused: true,
            ..default()
        })
        .id();
    app.add_systems(Update, rr_game::player::grab_cursor);
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    app.update();
    // A fresh Left press at the start of the frame the menu action runs in.
    app.add_systems(
        Update,
        (|mut m: ResMut<ButtonInput<MouseButton>>| {
            m.release(MouseButton::Left);
            m.press(MouseButton::Left);
        })
        .before(rr_game::player::grab_cursor),
    );
    press(&mut app, MenuAction::Resume);
    assert_eq!(state(&app), PlayState::Playing);
    let grab = app.world().get::<CursorOptions>(cursor).unwrap().grab_mode;
    assert_eq!(grab, CursorGrabMode::None, "the menu click grabbed");
    // And the next click does grab.
    app.update();
    let grab = app.world().get::<CursorOptions>(cursor).unwrap().grab_mode;
    assert_ne!(grab, CursorGrabMode::None, "the next click grabs");
    let mut q = app.world_mut().query::<&PendingInput>();
    assert!(!q.single(app.world()).unwrap().fire);
}
