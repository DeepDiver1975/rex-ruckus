//! Menus: main, difficulty, pause, options, controls and the stats screens, drawn with
//! `bevy_ui` over the frozen level.
//!
//! Every way of choosing a button (mouse click, Enter) ends in [`MenuInput::activate`]; the one
//! exclusive system [`run_menu_action`] performs it, so tests can drive the menus without a
//! window by writing that field. [`MenuScreen`] follows [`PlayState`] and is changed further by
//! actions (sub-screens). A new screen needs a [`Screen`] variant, a spawn function called from
//! [`rebuild_screen`] and, if it has a back target, an arm in [`Screen::back`].

pub mod controls;
pub mod main_menu;
pub mod options;
pub mod pause;
pub mod stats;
pub mod widgets;

pub use controls::{Capture, Notice};

use crate::bindings::{Action, Bindings};
use crate::episode::{Episode, Stats, start_episode};
use crate::flow::{
    AdvanceRequested, LevelDifficulty, PausedFrom, PlayState, RESTART_DELAY, StateAge, load_level,
    restart_level,
};
use crate::hud::UiFont;
use crate::level::CurrentMap;
use crate::player::{grab_cursor, pause_on_escape};
use crate::settings::Settings;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;

/// A menu screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    Difficulty,
    Pause,
    Options,
    Controls,
    /// Level stats after a completed level of an episode.
    Stats,
    /// Episode totals after the last level.
    EpisodeEnd,
}

impl Screen {
    /// The screen Esc/Back returns to; `None` for a top-level screen.
    pub fn back(self, state: PlayState) -> Option<Screen> {
        match self {
            Screen::Difficulty => Some(Screen::Main),
            Screen::Options if state == PlayState::Paused => Some(Screen::Pause),
            Screen::Options => Some(Screen::Main),
            Screen::Controls => Some(Screen::Options),
            Screen::Main | Screen::Pause | Screen::Stats | Screen::EpisodeEnd => None,
        }
    }

    /// The screen a play state shows by default (`has_episode`: an episode is being played).
    pub fn for_state(state: PlayState, has_episode: bool) -> Option<Screen> {
        match state {
            PlayState::Menu => Some(Screen::Main),
            PlayState::Paused => Some(Screen::Pause),
            PlayState::Complete if has_episode => Some(Screen::Stats),
            PlayState::EpisodeEnd => Some(Screen::EpisodeEnd),
            PlayState::Playing | PlayState::Dead | PlayState::Complete => None,
        }
    }
}

/// A player setting an options row can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Sensitivity,
    InvertY,
    Fov,
    Master,
    Sfx,
    Voice,
    Music,
    LowRes,
}

/// What a menu button does; attached to the button entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum MenuAction {
    NewGame,
    PickDifficulty(Difficulty),
    Resume,
    Options,
    Controls,
    RestartLevel,
    QuitToMenu,
    QuitGame,
    Back,
    /// Leave the stats screen for the next level.
    Continue,
    Rebind(Action),
    ResetBindings,
    Adjust(Setting, i8),
    Toggle(Setting),
}

/// The screen being shown, if any.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct MenuScreen(pub Option<Screen>);

/// The chosen action, performed (and cleared) by [`run_menu_action`].
#[derive(Resource, Default, Debug)]
pub struct MenuInput {
    pub activate: Option<MenuAction>,
}

/// Index of the highlighted button, in spawn order.
#[derive(Resource, Default, Debug)]
pub struct MenuSelection(pub usize);

/// The full-screen root of the screen being shown.
#[derive(Component)]
pub struct MenuRoot;

/// Moves a selection through `len` rows, wrapping at both ends.
pub fn navigate(selected: usize, len: usize, up: bool, down: bool) -> usize {
    if len == 0 {
        0
    } else if up {
        (selected + len - 1) % len
    } else if down {
        (selected + 1) % len
    } else {
        selected.min(len - 1)
    }
}

pub struct MenuPlugin {
    /// Off for scripted demos: no menus, no systems.
    pub enabled: bool,
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        if !self.enabled {
            return;
        }
        app.init_resource::<MenuScreen>()
            .init_resource::<MenuInput>()
            .init_resource::<MenuSelection>()
            .init_resource::<Capture>()
            .init_resource::<Notice>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Bindings>()
            .init_resource::<UiFont>()
            .add_systems(
                Update,
                (
                    // After the pause toggle, so Esc that goes back a screen is not also a resume.
                    keyboard_nav.after(pause_on_escape),
                    options::arrow_keys,
                    click_buttons,
                    options::click_arrows,
                    // After the cursor grab: a click on a menu button is consumed by the menu
                    // (the grab sees the menu still open), and the next click grabs.
                    run_menu_action.after(grab_cursor),
                    // After the action: the frame a capture starts is skipped.
                    controls::capture_input,
                    sync_screen,
                    rebuild_screen.run_if(resource_changed::<MenuScreen>),
                    options::refresh_values,
                    controls::refresh_rows,
                    options::save_on_leave,
                    highlight,
                )
                    .chain(),
            );
    }
}

/// Finds the buttons of the screen on show.
#[derive(SystemParam)]
struct MenuButtons<'w, 's> {
    roots: Query<'w, 's, Entity, With<MenuRoot>>,
    children: Query<'w, 's, &'static Children>,
    actions: Query<'w, 's, &'static MenuAction>,
}

impl MenuButtons<'_, '_> {
    /// The buttons in spawn order.
    fn list(&self) -> Vec<(Entity, MenuAction)> {
        self.roots
            .iter()
            .flat_map(|root| self.children.iter_descendants(root))
            .filter_map(|e| self.actions.get(e).ok().map(|a| (e, *a)))
            .collect()
    }
}

/// Up/Down (or W/S) move the selection, Enter/Space chooses, Esc goes back a screen.
#[allow(clippy::too_many_arguments)]
fn keyboard_nav(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    capture: Res<Capture>,
    state: Res<PlayState>,
    buttons: MenuButtons,
    mut selection: ResMut<MenuSelection>,
    mut input: ResMut<MenuInput>,
    mut screen: ResMut<MenuScreen>,
) {
    let Some(current) = screen.0 else { return };
    if capture.0.is_some() {
        return;
    }
    let items = buttons.list();
    let up = keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]);
    let down = keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]);
    let moved = navigate(selection.0, items.len(), up, down);
    if moved != selection.0 {
        selection.0 = moved;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space])
        && let Some((_, action)) = items.get(selection.0)
    {
        input.activate = Some(*action);
    }
    if bindings.just_pressed(Action::Pause, &keys, &mouse)
        && let Some(back) = current.back(*state)
    {
        screen.0 = Some(back);
    }
}

/// A click chooses a button; hovering selects it.
fn click_buttons(
    changed: Query<(Entity, &Interaction, &MenuAction), Changed<Interaction>>,
    buttons: MenuButtons,
    capture: Res<Capture>,
    mut selection: ResMut<MenuSelection>,
    mut input: ResMut<MenuInput>,
) {
    if changed.is_empty() || capture.0.is_some() {
        return;
    }
    let items = buttons.list();
    for (entity, interaction, action) in &changed {
        match interaction {
            Interaction::Pressed => input.activate = Some(*action),
            Interaction::Hovered => {
                if let Some(i) = items.iter().position(|(e, _)| *e == entity) {
                    selection.0 = i;
                }
            }
            Interaction::None => {}
        }
    }
}

/// Performs the chosen action.
pub fn run_menu_action(world: &mut World) {
    let Some(action) = world.resource_mut::<MenuInput>().activate.take() else {
        return;
    };
    // Waiting for a key: choosing a button does nothing.
    if world.resource::<Capture>().0.is_some() {
        return;
    }
    let state = *world.resource::<PlayState>();
    // The stats screens stay up for a moment: Enter, Space (Jump) or a click that was meant for
    // the game must not skip them unseen.
    let skips_stats = match action {
        MenuAction::Continue => true,
        MenuAction::QuitToMenu => state == PlayState::EpisodeEnd,
        _ => false,
    };
    if skips_stats && world.resource::<StateAge>().0 < RESTART_DELAY {
        return;
    }
    let screen = |world: &mut World, s: Option<Screen>| world.resource_mut::<MenuScreen>().0 = s;
    match action {
        MenuAction::NewGame => screen(world, Some(Screen::Difficulty)),
        MenuAction::PickDifficulty(d) => {
            if world.contains_resource::<Episode>() {
                // Loads level 0 and sets `Playing`.
                start_episode(world, d);
            }
        }
        MenuAction::Resume => {
            let from = world.resource::<PausedFrom>().0;
            *world.resource_mut::<PlayState>() = from;
        }
        MenuAction::RestartLevel => {
            // Rebuilds the level and sets `Playing`, so no tick of the old level runs.
            restart_level(world);
        }
        MenuAction::QuitToMenu => quit_to_menu(world),
        MenuAction::QuitGame => {
            world.write_message(AppExit::Success);
        }
        MenuAction::Options => screen(world, Some(Screen::Options)),
        MenuAction::Controls => {
            world.resource_mut::<Notice>().0.clear();
            screen(world, Some(Screen::Controls));
        }
        MenuAction::Back => {
            let current = world.resource::<MenuScreen>().0;
            if let Some(back) = current.and_then(|s| s.back(state)) {
                screen(world, Some(back));
            }
        }
        MenuAction::Rebind(a) => {
            world.resource_mut::<Notice>().0.clear();
            world.resource_mut::<Capture>().0 = Some(a);
        }
        MenuAction::ResetBindings => {
            world.resource_mut::<Notice>().0.clear();
            if let Some(mut settings) = world.get_resource_mut::<Settings>() {
                settings.bindings = Bindings::default();
            }
        }
        MenuAction::Adjust(which, dir) => {
            if let Some(mut settings) = world.get_resource_mut::<Settings>() {
                options::adjust(&mut settings, which, dir);
            }
        }
        MenuAction::Toggle(which) => {
            if let Some(mut settings) = world.get_resource_mut::<Settings>() {
                options::adjust(&mut settings, which, 1);
            }
        }
        MenuAction::Continue => {
            // Only from the stats screen: a Fire press that already advanced the level (the
            // click carries one) must not skip the next level as well.
            if state == PlayState::Complete && world.contains_resource::<Episode>() {
                world.resource_mut::<AdvanceRequested>().0 = true;
            }
        }
    }
}

/// Back to the title screen: level 0 of the episode, frozen, with the starting loadout.
fn quit_to_menu(world: &mut World) {
    if !world.contains_resource::<Episode>() {
        return;
    }
    world.insert_resource(PausedFrom::default());
    let Some(mut episode) = world.get_resource_mut::<Episode>() else {
        return;
    };
    episode.index = 0;
    episode.entry = None;
    episode.totals = default();
    let map = episode.map(0);
    load_level(world, map);
    *world.resource_mut::<PlayState>() = PlayState::Menu;
}

/// Shows the default screen of a new play state.
fn sync_screen(
    state: Res<PlayState>,
    episode: Option<Res<Episode>>,
    mut screen: ResMut<MenuScreen>,
) {
    if !state.is_changed() {
        return;
    }
    let wanted = Screen::for_state(*state, episode.is_some());
    if screen.0 != wanted {
        screen.0 = wanted;
    }
}

/// Replaces the menu entities when the screen changes.
#[allow(clippy::too_many_arguments)]
fn rebuild_screen(
    mut commands: Commands,
    screen: Res<MenuScreen>,
    ui: Res<UiFont>,
    episode: Option<Res<Episode>>,
    stats: Res<Stats>,
    map: Option<Res<CurrentMap>>,
    difficulty: Res<LevelDifficulty>,
    paused_from: Res<PausedFrom>,
    roots: Query<Entity, With<MenuRoot>>,
    mut selection: ResMut<MenuSelection>,
) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    selection.0 = 0;
    if screen.0 == Some(Screen::Difficulty) {
        // Easy, Normal, Hard in button order; `--difficulty` or the last game's pick is pre-selected.
        selection.0 = match difficulty.0 {
            Difficulty::Easy => 0,
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
        };
    }
    match screen.0 {
        Some(Screen::Main) => main_menu::spawn_main(&mut commands, &ui),
        Some(Screen::Difficulty) => main_menu::spawn_difficulty(&mut commands, &ui),
        Some(Screen::Pause) => pause::spawn_pause(
            &mut commands,
            &ui,
            episode.is_some(),
            paused_from.0 == PlayState::Playing,
        ),
        Some(Screen::Options) => options::spawn_options(&mut commands, &ui),
        Some(Screen::Controls) => controls::spawn_controls(&mut commands, &ui),
        Some(Screen::Stats) => {
            let name = map.as_ref().map_or("", |m| m.0.name.as_str());
            stats::spawn_stats(&mut commands, &ui, name, &stats.0);
        }
        Some(Screen::EpisodeEnd) => {
            let totals = episode.map(|e| e.totals).unwrap_or_default();
            stats::spawn_episode_end(&mut commands, &ui, difficulty.0, &totals);
        }
        None => {}
    }
}

/// Colours the selected button.
fn highlight(
    selection: Res<MenuSelection>,
    buttons: MenuButtons,
    mut colors: Query<&mut BackgroundColor, With<MenuAction>>,
) {
    for (i, (entity, _)) in buttons.list().into_iter().enumerate() {
        if let Ok(mut color) = colors.get_mut(entity) {
            let wanted = BackgroundColor(if i == selection.0 {
                widgets::SELECTED
            } else {
                widgets::IDLE
            });
            if *color != wanted {
                *color = wanted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_wraps() {
        assert_eq!(navigate(0, 3, true, false), 2);
        assert_eq!(navigate(2, 3, false, true), 0);
        assert_eq!(navigate(1, 3, false, false), 1);
        assert_eq!(navigate(5, 0, true, false), 0);
        assert_eq!(navigate(4, 2, false, false), 1);
    }

    #[test]
    fn back_targets() {
        assert_eq!(Screen::Difficulty.back(PlayState::Menu), Some(Screen::Main));
        assert_eq!(Screen::Options.back(PlayState::Menu), Some(Screen::Main));
        assert_eq!(Screen::Options.back(PlayState::Paused), Some(Screen::Pause));
        assert_eq!(
            Screen::Controls.back(PlayState::Paused),
            Some(Screen::Options)
        );
        assert_eq!(Screen::Pause.back(PlayState::Paused), None);
    }

    #[test]
    fn states_pick_screens() {
        use PlayState::*;
        assert_eq!(Screen::for_state(Menu, false), Some(Screen::Main));
        assert_eq!(Screen::for_state(Paused, false), Some(Screen::Pause));
        assert_eq!(Screen::for_state(Complete, true), Some(Screen::Stats));
        assert_eq!(Screen::for_state(Complete, false), None);
        assert_eq!(Screen::for_state(Playing, true), None);
        assert_eq!(Screen::for_state(Dead, true), None);
    }
}
