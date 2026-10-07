//! The menus drive the game flow (headless: actions are injected through `MenuInput`).

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use rr_core::defs::Defs;
use rr_core::difficulty::Difficulty;
use rr_core::fixtures::combat_room;
use rr_core::map::Map;
use rr_game::bindings::{Action, Binding};
use rr_game::combat::{CombatSimPlugin, insert_defs};
use rr_game::episode::{Episode, EpisodeDef, EpisodePlugin};
use rr_game::flow::{FlowPlugin, LevelDifficulty, PlayState, RESTART_DELAY, StateAge};
use rr_game::hud::{HudPlugin, HudRoot};
use rr_game::mechanics::{MechanicsSimPlugin, insert_level};
use rr_game::menu::{
    Capture, MenuAction, MenuInput, MenuPlugin, MenuRoot, MenuScreen, Notice, Screen, Setting,
};
use rr_game::player::{PendingInput, PlayerBody, PlayerSimPlugin};
use rr_game::settings::{Settings, SettingsPlugin};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The settings file of a test app.
#[derive(Resource)]
struct SettingsFile(PathBuf);

fn settings_file(app: &App) -> PathBuf {
    app.world().resource::<SettingsFile>().0.clone()
}

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
    static N: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rr-menu-sim-{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // A stale dir from a recycled pid must never leak files into this test.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.ron");
    app.insert_resource(SettingsFile(path.clone()));
    app.add_plugins(SettingsPlugin { path: Some(path) });
    app.add_plugins((
        FlowPlugin,
        PlayerSimPlugin,
        MechanicsSimPlugin,
        CombatSimPlugin,
        EpisodePlugin,
        HudPlugin,
        MenuPlugin { enabled: true },
    ));
    // The headless player plugin leaves the pause key out; Esc tests need it.
    app.add_systems(Update, rr_game::player::pause_on_escape);
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

fn open_options(app: &mut App) {
    press(app, MenuAction::Options);
    assert_eq!(screen(app), Some(Screen::Options));
}

fn open_controls(app: &mut App) {
    open_options(app);
    press(app, MenuAction::Controls);
    assert_eq!(screen(app), Some(Screen::Controls));
}

/// Presses and releases a key over one frame.
fn key(app: &mut App, k: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(k);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(k);
    keys.clear();
}

#[test]
fn rebinding_captures_the_next_key_and_esc_cancels() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    press(&mut app, MenuAction::Rebind(Action::Jump));
    assert_eq!(app.world().resource::<Capture>().0, Some(Action::Jump));
    key(&mut app, KeyCode::Escape);
    assert_eq!(app.world().resource::<Capture>().0, None);
    assert_eq!(screen(&app), Some(Screen::Controls), "Esc only cancels");
    assert_eq!(
        app.world().resource::<Settings>().bindings.of(Action::Jump),
        &[Binding::Key(KeyCode::Space)]
    );
    press(&mut app, MenuAction::Rebind(Action::Jump));
    key(&mut app, KeyCode::KeyE);
    let b = &app.world().resource::<Settings>().bindings;
    assert_eq!(b.of(Action::Jump)[0], Binding::Key(KeyCode::KeyE));
    assert!(b.of(Action::Use).is_empty());
    assert_eq!(app.world().resource::<Capture>().0, None);
}

#[test]
fn the_click_that_starts_a_capture_is_not_captured() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    // The mouse press that chose the row is still "just pressed" in the frame the capture starts.
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    press(&mut app, MenuAction::Rebind(Action::Jump));
    assert_eq!(app.world().resource::<Capture>().0, Some(Action::Jump));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    // A later mouse press binds.
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    assert_eq!(
        app.world().resource::<Settings>().bindings.of(Action::Jump)[0],
        Binding::Mouse(MouseButton::Right)
    );
}

#[test]
fn menu_input_is_ignored_while_capturing() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    press(&mut app, MenuAction::Rebind(Action::Jump));
    press(&mut app, MenuAction::Back);
    assert_eq!(screen(&app), Some(Screen::Controls));
    assert_eq!(app.world().resource::<Capture>().0, Some(Action::Jump));
}

#[test]
fn reset_restores_the_default_bindings() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    press(&mut app, MenuAction::Rebind(Action::Jump));
    key(&mut app, KeyCode::KeyE);
    press(&mut app, MenuAction::ResetBindings);
    assert_eq!(
        app.world().resource::<Settings>().bindings,
        rr_game::bindings::Bindings::default()
    );
}

#[test]
fn leaving_options_saves() {
    let mut app = menu_app(vec![combat_room()]);
    open_options(&mut app);
    press(&mut app, MenuAction::Adjust(Setting::Fov, 1));
    press(&mut app, MenuAction::Back);
    app.update(); // save_settings runs on the frame after the flag is set
    assert_eq!(Settings::load_from(&settings_file(&app)).0.fov_deg, 80.0);
}

#[test]
fn arrow_keys_adjust_the_selected_row() {
    let mut app = menu_app(vec![combat_room()]);
    open_options(&mut app);
    // Row 0 is the sensitivity.
    key(&mut app, KeyCode::ArrowRight);
    let sens = app.world().resource::<Settings>().mouse_sensitivity;
    assert!(sens > Settings::default().mouse_sensitivity);
    key(&mut app, KeyCode::ArrowLeft);
    key(&mut app, KeyCode::ArrowLeft);
    assert!(
        app.world().resource::<Settings>().mouse_sensitivity
            < Settings::default().mouse_sensitivity
    );
}

/// Starts the episode on Normal and ends the current level.
fn complete_level(app: &mut App) {
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Complete;
    app.update();
}

fn start(app: &mut App) {
    press(app, MenuAction::NewGame);
    press(app, MenuAction::PickDifficulty(Difficulty::Normal));
}

fn index(app: &App) -> usize {
    app.world().resource::<Episode>().index
}

/// Chooses Continue and lets the fixed tick that performs the advance run.
fn press_continue(app: &mut App) {
    // The screen has been up long enough.
    app.world_mut().resource_mut::<StateAge>().0 = RESTART_DELAY;
    press(app, MenuAction::Continue);
    app.world_mut().run_schedule(FixedUpdate);
    app.update();
}

#[test]
fn stats_then_continue_through_the_episode() {
    let mut app = menu_app(vec![combat_room(), combat_room()]);
    start(&mut app);
    complete_level(&mut app);
    assert_eq!(screen(&app), Some(Screen::Stats));
    assert_eq!(roots(&mut app), 1);
    press_continue(&mut app);
    assert_eq!(index(&app), 1);
    assert_eq!(state(&app), PlayState::Playing);
    complete_level(&mut app);
    assert_eq!(screen(&app), Some(Screen::Stats));
    press_continue(&mut app);
    assert_eq!(state(&app), PlayState::EpisodeEnd);
    app.update();
    assert_eq!(screen(&app), Some(Screen::EpisodeEnd));
    assert_eq!(roots(&mut app), 1);
    app.world_mut().resource_mut::<StateAge>().0 = RESTART_DELAY;
    press(&mut app, MenuAction::QuitToMenu);
    assert_eq!(state(&app), PlayState::Menu);
    app.update();
    assert_eq!(screen(&app), Some(Screen::Main));
}

/// A click on Continue must not also advance through the Fire press it carries.
#[test]
fn one_press_advances_exactly_one_level() {
    let mut app = menu_app(vec![combat_room(), combat_room(), combat_room()]);
    start(&mut app);
    complete_level(&mut app);
    // The age resets on the first tick that sees the new state.
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut().resource_mut::<rr_game::flow::StateAge>().0 = 5.0;
    // Continue chosen in the same frame as a Fire press that the tick has not consumed yet.
    app.world_mut()
        .query::<&mut PendingInput>()
        .single_mut(app.world_mut())
        .unwrap()
        .fire_pressed = true;
    press(&mut app, MenuAction::Continue);
    for _ in 0..4 {
        app.world_mut().run_schedule(FixedUpdate);
        app.update();
    }
    assert_eq!(index(&app), 1, "skipped a level");
    assert_eq!(state(&app), PlayState::Playing);
}

/// The Fire press of the click advances in the fixed tick before the menu action runs: the
/// Continue that follows must not advance again.
#[test]
fn continue_after_a_fire_advance_is_ignored() {
    let mut app = menu_app(vec![combat_room(), combat_room(), combat_room()]);
    start(&mut app);
    complete_level(&mut app);
    // The age resets on the first tick that sees the new state.
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut().resource_mut::<rr_game::flow::StateAge>().0 = 5.0;
    app.world_mut()
        .query::<&mut PendingInput>()
        .single_mut(app.world_mut())
        .unwrap()
        .fire_pressed = true;
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(index(&app), 1);
    press_continue(&mut app);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(index(&app), 1, "skipped a level");
}

#[test]
fn continue_outside_the_stats_screen_does_nothing() {
    let mut app = menu_app(vec![combat_room(), combat_room()]);
    start(&mut app);
    press(&mut app, MenuAction::Continue);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(index(&app), 0);
}

#[test]
fn rebind_notice_names_the_moved_binding_and_clears() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    press(&mut app, MenuAction::Rebind(Action::Jump));
    key(&mut app, KeyCode::KeyE);
    assert_eq!(app.world().resource::<Notice>().0, "E moved from Use");
    press(&mut app, MenuAction::Rebind(Action::Jump));
    assert_eq!(app.world().resource::<Notice>().0, "");
    // Esc cancelling a capture clears a notice too.
    app.world_mut().resource_mut::<Notice>().0 = "stale".into();
    key(&mut app, KeyCode::Escape);
    assert_eq!(app.world().resource::<Capture>().0, None);
    assert_eq!(app.world().resource::<Notice>().0, "");
}

/// The menu dims the world: a full-window, mostly opaque backdrop drawn above the HUD layers.
#[test]
fn the_main_menu_dims_the_whole_window() {
    use rr_game::menu::widgets::{BACKDROP, MENU_Z};
    let mut app = menu_app(vec![combat_room()]);
    let mut q = app
        .world_mut()
        .query_filtered::<(&Node, &BackgroundColor, &GlobalZIndex), With<MenuRoot>>();
    let (node, bg, z) = q.single(app.world()).unwrap();
    assert_eq!(node.width, Val::Percent(100.0));
    assert_eq!(node.height, Val::Percent(100.0));
    assert_eq!(node.position_type, PositionType::Absolute);
    assert_eq!(bg.0, BACKDROP);
    assert!(bg.0.alpha() >= 0.75, "too transparent to dim the level");
    assert!(z.0 >= MENU_Z && MENU_Z > 10, "below the HUD");
}

#[test]
fn left_click_cancels_a_capture_without_binding() {
    let mut app = menu_app(vec![combat_room()]);
    open_controls(&mut app);
    let before = app.world().resource::<Settings>().bindings.clone();
    press(&mut app, MenuAction::Rebind(Action::Jetpack));
    app.update(); // armed
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(app.world().resource::<Capture>().0, None);
    assert_eq!(app.world().resource::<Settings>().bindings, before);
    assert_eq!(screen(&app), Some(Screen::Controls));
}

#[test]
fn stats_screens_cannot_be_skipped_at_once() {
    let mut app = menu_app(vec![combat_room(), combat_room()]);
    start(&mut app);
    complete_level(&mut app);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(state(&app), PlayState::Complete);
    // At age 0 Continue does nothing.
    app.world_mut().resource_mut::<StateAge>().0 = 0.0;
    press(&mut app, MenuAction::Continue);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!((state(&app), index(&app)), (PlayState::Complete, 0));
    // Past the delay it advances.
    press_continue(&mut app);
    assert_eq!((state(&app), index(&app)), (PlayState::Playing, 1));
    // The episode-end "Main menu" is held back the same way.
    *app.world_mut().resource_mut::<PlayState>() = PlayState::EpisodeEnd;
    app.update();
    app.world_mut().resource_mut::<StateAge>().0 = 0.0;
    press(&mut app, MenuAction::QuitToMenu);
    assert_eq!(state(&app), PlayState::EpisodeEnd);
    app.world_mut().resource_mut::<StateAge>().0 = RESTART_DELAY;
    press(&mut app, MenuAction::QuitToMenu);
    assert_eq!(state(&app), PlayState::Menu);
}

/// The HUD hides while any menu screen is up and returns with play.
#[test]
fn the_hud_hides_under_menus() {
    let mut app = menu_app(vec![combat_room()]);
    app.update();
    let visibilities = |app: &mut App| -> Vec<Visibility> {
        app.world_mut()
            .query_filtered::<&Visibility, With<HudRoot>>()
            .iter(app.world())
            .copied()
            .collect()
    };
    let v = visibilities(&mut app);
    assert!(!v.is_empty());
    assert!(v.iter().all(|v| *v == Visibility::Hidden), "main menu");
    start(&mut app);
    app.update();
    assert!(
        visibilities(&mut app)
            .iter()
            .all(|v| *v != Visibility::Hidden)
    );
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Paused;
    // One frame for the screen to follow the state, one for the HUD to follow the screen.
    app.update();
    app.update();
    assert!(
        visibilities(&mut app)
            .iter()
            .all(|v| *v == Visibility::Hidden)
    );
}

/// Losing the window focus while playing pauses; a focused window or a menu state does not.
#[test]
fn focus_loss_pauses_while_playing() {
    let mut app = menu_app(vec![combat_room()]);
    start(&mut app);
    let window = app
        .world_mut()
        .spawn(Window {
            focused: true,
            ..default()
        })
        .id();
    app.add_systems(Update, rr_game::player::pause_on_focus_loss);
    app.update();
    assert_eq!(state(&app), PlayState::Playing);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_eq!(state(&app), PlayState::Paused);
}

/// Back at the title after an episode, a new game starts from scratch on the picked skill.
#[test]
fn new_game_after_the_episode_end_starts_fresh() {
    let mut app = menu_app(vec![combat_room(), combat_room()]);
    start(&mut app);
    complete_level(&mut app);
    press_continue(&mut app);
    complete_level(&mut app);
    press_continue(&mut app);
    assert_eq!(state(&app), PlayState::EpisodeEnd);
    app.update();
    app.world_mut().resource_mut::<Episode>().totals.kills = 7;
    app.world_mut().resource_mut::<StateAge>().0 = RESTART_DELAY;
    press(&mut app, MenuAction::QuitToMenu);
    app.update();
    assert_eq!(screen(&app), Some(Screen::Main));
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Hard));
    let ep = app.world().resource::<Episode>();
    assert_eq!(ep.index, 0);
    assert_eq!(ep.totals, Default::default());
    assert!(ep.entry.is_none());
    assert_eq!(state(&app), PlayState::Playing);
    assert_eq!(
        app.world().resource::<LevelDifficulty>().0,
        Difficulty::Hard
    );
}

#[test]
fn esc_while_dead_opens_pause_and_returns_to_dead() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    *app.world_mut().resource_mut::<PlayState>() = PlayState::Dead;
    app.update();
    key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), PlayState::Paused);
    app.update();
    assert_eq!(screen(&app), Some(Screen::Pause));
    // No "Resume" button: resuming a corpse makes no sense.
    let has_resume = app
        .world_mut()
        .query::<&MenuAction>()
        .iter(app.world())
        .any(|a| *a == MenuAction::Resume);
    assert!(!has_resume);
    key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), PlayState::Dead);
}

#[test]
fn pause_from_playing_still_resumes_to_playing() {
    let mut app = menu_app(vec![combat_room()]);
    press(&mut app, MenuAction::NewGame);
    press(&mut app, MenuAction::PickDifficulty(Difficulty::Normal));
    key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), PlayState::Paused);
    press(&mut app, MenuAction::Resume);
    assert_eq!(state(&app), PlayState::Playing);
}
