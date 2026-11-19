//! Menus: main, episode, difficulty, pause, options, controls, the stats screens and the co-op
//! screens (`multiplayer`), drawn with `bevy_ui` over the frozen level (or, in a net game, over
//! the running one).
//!
//! Every way of choosing a button (mouse click, Enter) ends in [`MenuInput::activate`]; the one
//! exclusive system [`run_menu_action`] performs it, so tests can drive the menus without a
//! window by writing that field. [`MenuScreen`] follows [`PlayState`] and is changed further by
//! actions (sub-screens). A new screen needs a [`Screen`] variant, a spawn function called from
//! [`rebuild_screen`] and, if it has a back target, an arm in [`Screen::back`].

pub mod controls;
pub mod main_menu;
pub mod multiplayer;
pub mod options;
pub mod pause;
pub mod stats;
pub mod text_field;
pub mod widgets;

pub use controls::{Capture, Notice};
pub use multiplayer::HostSetup;
pub use text_field::{Editing, Field};

use crate::bindings::{Action, Bindings};
use crate::episode::{Episode, Stats, start_episode};
use crate::flow::{
    AdvanceRequested, LevelDifficulty, PausedFrom, PlayState, RESTART_DELAY, StateAge, load_level,
    restart_level,
};
use crate::hud::UiFont;
use crate::level::CurrentMap;
use crate::net::NetMode;
use crate::net::client::{HostRun, NetNotice};
use crate::net::lobby::{LanGames, browse_lan};
use crate::player::{grab_cursor, pause_on_escape};
use crate::settings::{Settings, SettingsPath};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;

/// A menu screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    /// The episode picker shown after New Game.
    Episodes,
    Difficulty,
    Pause,
    Options,
    Controls,
    /// Level stats after a completed level of an episode.
    Stats,
    /// Episode totals after the last level.
    EpisodeEnd,
    /// Name, Host game, Join game.
    Multiplayer,
    /// LAN games and an address to join.
    Join,
    /// A co-op game before its level starts: level, skill and players.
    Lobby,
    /// Why the last co-op session ended (`NetNotice`).
    NetNotice,
}

impl Screen {
    /// The screen Esc/Back returns to; `None` for a top-level screen.
    pub fn back(self, state: PlayState) -> Option<Screen> {
        match self {
            Screen::Episodes => Some(Screen::Main),
            Screen::Difficulty => Some(Screen::Episodes),
            Screen::Options if state == PlayState::Paused => Some(Screen::Pause),
            Screen::Options => Some(Screen::Main),
            Screen::Controls => Some(Screen::Options),
            Screen::Multiplayer => Some(Screen::Main),
            Screen::Join => Some(Screen::Multiplayer),
            Screen::Main
            | Screen::Pause
            | Screen::Stats
            | Screen::EpisodeEnd
            | Screen::Lobby
            | Screen::NetNotice => None,
        }
    }

    /// [`Screen::back`], except that the episode picker of a game being set up for hosting goes
    /// back to the Multiplayer screen.
    pub fn back_from(self, state: PlayState, host_setup: bool) -> Option<Screen> {
        match self {
            Screen::Episodes if host_setup => Some(Screen::Multiplayer),
            _ => self.back(state),
        }
    }

    /// The screen a play state shows by default (`stats_screens`: an episode is being played,
    /// here or by the host, see [`stats_screens`]).
    pub fn for_state(state: PlayState, stats_screens: bool) -> Option<Screen> {
        match state {
            PlayState::Menu => Some(Screen::Main),
            PlayState::Paused => Some(Screen::Pause),
            PlayState::Complete if stats_screens => Some(Screen::Stats),
            PlayState::EpisodeEnd => Some(Screen::EpisodeEnd),
            PlayState::Playing | PlayState::Dead | PlayState::Complete => None,
        }
    }
}

/// Whether a completed level shows the stats screen: in an episode. A co-op client goes by the
/// host's run (`run`), not by the title screen's episode it keeps for after the session.
pub fn stats_screens(mode: NetMode, episode: bool, run: Option<&HostRun>) -> bool {
    match mode {
        NetMode::Client => run.is_some_and(|r| r.episode),
        NetMode::Single | NetMode::Host => episode,
    }
}

/// [`stats_screens`] of `world`.
pub fn stats_screens_in(world: &World) -> bool {
    stats_screens(
        world.get_resource::<NetMode>().copied().unwrap_or_default(),
        world.contains_resource::<Episode>(),
        world.get_resource::<HostRun>(),
    )
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
    /// Index into `Episode::choices`.
    PickEpisode(usize),
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
    Multiplayer,
    /// Episode and difficulty, then the Lobby of a hosted game.
    HostGame,
    JoinGame,
    /// Join a LAN game.
    JoinLan(std::net::SocketAddr),
    /// Join the typed address.
    Connect,
    /// The host starts the level for everyone.
    StartGame,
    /// Leave the Lobby.
    Leave,
    /// Leave a net game from the pause menu.
    LeaveGame,
    NoticeOk,
    /// Start (or end) typing into a field.
    Edit(Field),
}

/// Whether keys are being typed rather than pressed: a rebind waits for its key, or a text field
/// is being edited. Navigation, pause and the fixed sound keys stay out of the way meanwhile.
#[derive(SystemParam)]
pub struct KeysTyped<'w> {
    capture: Option<Res<'w, Capture>>,
    editing: Option<Res<'w, Editing>>,
}

impl KeysTyped<'_> {
    pub fn active(&self) -> bool {
        self.capture.as_ref().is_some_and(|c| c.0.is_some())
            || self.editing.as_ref().is_some_and(|e| e.0.is_some())
    }
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
            .init_resource::<Editing>()
            .init_resource::<HostSetup>()
            .init_resource::<multiplayer::HostPort>()
            .init_resource::<text_field::JoinAddress>()
            .init_resource::<LanGames>()
            .init_resource::<NetNotice>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
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
                    text_field::text_input,
                    sync_screen,
                    // After the state's default screen: a notice wins over the title screen.
                    multiplayer::show_notice,
                    browse_lan,
                    rebuild_screen.run_if(resource_changed::<MenuScreen>),
                    options::refresh_values,
                    controls::refresh_rows,
                    text_field::refresh_fields,
                    multiplayer::refresh_texts,
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
    (typed, host_setup): (KeysTyped, Res<HostSetup>),
    state: Res<PlayState>,
    buttons: MenuButtons,
    mut selection: ResMut<MenuSelection>,
    mut input: ResMut<MenuInput>,
    mut screen: ResMut<MenuScreen>,
) {
    let Some(current) = screen.0 else { return };
    // Keys go to a rebind or a text field instead.
    if typed.active() {
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
        && let Some(back) = current.back_from(*state, host_setup.0)
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
        MenuAction::QuitToMenu | MenuAction::LeaveGame => state == PlayState::EpisodeEnd,
        _ => false,
    };
    if skips_stats && world.resource::<StateAge>().0 < RESTART_DELAY {
        return;
    }
    // Any button ends typing into a field.
    text_field::edit_action(world, action);
    let screen = |world: &mut World, s: Option<Screen>| world.resource_mut::<MenuScreen>().0 = s;
    match action {
        MenuAction::NewGame => {
            world.resource_mut::<HostSetup>().0 = false;
            screen(world, Some(Screen::Episodes));
        }
        MenuAction::PickEpisode(i) => {
            let chosen = world
                .get_resource_mut::<Episode>()
                .is_some_and(|mut ep| ep.choose(i));
            if chosen {
                screen(world, Some(Screen::Difficulty));
            }
        }
        MenuAction::PickDifficulty(d) => {
            if world.resource::<HostSetup>().0 {
                multiplayer::host_game(world, d);
            } else if world.contains_resource::<Episode>() {
                // Loads level 0 and sets `Playing`.
                start_episode(world, d);
            }
        }
        MenuAction::Resume => {
            let from = world.resource::<PausedFrom>().0;
            *world.resource_mut::<PlayState>() = from;
        }
        MenuAction::RestartLevel => {
            // Rebuilds the level and sets `Playing`, so no tick of the old level runs. A co-op
            // client never restarts: the host decides.
            let mode = world.get_resource::<NetMode>().copied().unwrap_or_default();
            if mode != NetMode::Client {
                restart_level(world);
            }
        }
        MenuAction::QuitToMenu => {
            // A net game ends first: otherwise the title screen would still be hosting (or
            // joined), and the host's level-0 reload would send the clients a `Load`.
            multiplayer::stop_session(world);
            quit_to_menu(world);
        }
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
            let host_setup = world.resource::<HostSetup>().0;
            if let Some(back) = current.and_then(|s| s.back_from(state, host_setup)) {
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
            // click carries one) must not skip the next level as well. A co-op client never
            // moves on by itself: the host does, and its `Load` brings the next level.
            let client = world.get_resource::<NetMode>() == Some(&NetMode::Client);
            if state == PlayState::Complete && world.contains_resource::<Episode>() && !client {
                world.resource_mut::<AdvanceRequested>().0 = true;
            }
        }
        MenuAction::Multiplayer => screen(world, Some(Screen::Multiplayer)),
        MenuAction::HostGame => {
            world.resource_mut::<HostSetup>().0 = true;
            screen(world, Some(Screen::Episodes));
        }
        MenuAction::JoinGame => screen(world, Some(Screen::Join)),
        MenuAction::JoinLan(addr) => multiplayer::join(world, addr),
        MenuAction::Connect => multiplayer::connect(world),
        MenuAction::StartGame => multiplayer::start_game(world),
        MenuAction::Leave => multiplayer::leave_lobby(world),
        MenuAction::LeaveGame => multiplayer::leave_game(world),
        MenuAction::NoticeOk => multiplayer::notice_ok(world),
        // Handled by `edit_action` above.
        MenuAction::Edit(_) => {}
    }
}

/// Back to the title screen: level 0 of the episode, frozen, with the starting loadout.
pub(crate) fn quit_to_menu(world: &mut World) {
    if !world.contains_resource::<Episode>() {
        return;
    }
    world.insert_resource(PausedFrom::default());
    let Some(mut episode) = world.get_resource_mut::<Episode>() else {
        return;
    };
    episode.index = 0;
    episode.entries = Default::default();
    episode.totals = default();
    let (file, map) = (episode.file(0), episode.map(0));
    world.insert_resource(crate::flow::LevelFile(file));
    load_level(world, map);
    *world.resource_mut::<PlayState>() = PlayState::Menu;
}

/// Shows the default screen of a new play state; in a co-op session the menu state is its Lobby.
fn sync_screen(
    state: Res<PlayState>,
    episode: Option<Res<Episode>>,
    mode: Option<Res<NetMode>>,
    run: Option<Res<HostRun>>,
    paused_from: Res<PausedFrom>,
    mut screen: ResMut<MenuScreen>,
) {
    // A client's pause menu depends on where it was paused from, and that moves under it when
    // the host restarts (the death screen's pause has no Resume, the running game's has).
    let pause_moved = *state == PlayState::Paused && paused_from.is_changed();
    if !state.is_changed() && !pause_moved {
        return;
    }
    let mode = mode.as_deref().copied().unwrap_or_default();
    let net = mode.is_net();
    let stats = stats_screens(mode, episode.is_some(), run.as_deref());
    let wanted = match Screen::for_state(*state, stats) {
        Some(Screen::Main) if net => Some(Screen::Lobby),
        other => other,
    };
    if screen.0 != wanted {
        screen.0 = wanted;
    } else if pause_moved {
        screen.set_changed();
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
    settings_path: Option<Res<SettingsPath>>,
    net: multiplayer::NetView,
    roots: Query<Entity, With<MenuRoot>>,
    mut selection: ResMut<MenuSelection>,
) {
    let saved = settings_path.is_some_and(|p| p.0.is_some());
    for root in &roots {
        commands.entity(root).despawn();
    }
    selection.0 = 0;
    if screen.0 == Some(Screen::Episodes)
        && let Some(ep) = &episode
    {
        // The last chosen episode, counting only the playable ones (the others are no buttons).
        selection.0 = ep.choices[..ep.chosen]
            .iter()
            .filter(|d| d.playable())
            .count();
    }
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
        Some(Screen::Episodes) => {
            let choices = episode.as_ref().map_or(&[][..], |e| e.choices.as_slice());
            main_menu::spawn_episodes(&mut commands, &ui, choices);
        }
        Some(Screen::Difficulty) => main_menu::spawn_difficulty(&mut commands, &ui),
        Some(Screen::Pause) => pause::spawn_pause(
            &mut commands,
            &ui,
            episode.is_some(),
            paused_from.0 == PlayState::Playing,
            net.mode(),
        ),
        Some(Screen::Options) => options::spawn_options(&mut commands, &ui, saved),
        Some(Screen::Controls) => controls::spawn_controls(&mut commands, &ui),
        Some(Screen::Stats) => {
            let name = map.as_ref().map_or("", |m| m.0.name.as_str());
            let can_continue = net.mode() != NetMode::Client;
            stats::spawn_stats(
                &mut commands,
                &ui,
                name,
                &stats.0,
                &net.names(),
                can_continue,
            );
        }
        Some(Screen::EpisodeEnd) => {
            // A client shows the host's episode, not the one its title screen keeps.
            let client = net.mode() == NetMode::Client;
            let (name, totals) = if client {
                net.run()
                    .map(|r| (r.name.as_str(), r.totals))
                    .unwrap_or_default()
            } else {
                episode
                    .as_ref()
                    .map(|e| (e.def.name.as_str(), e.totals))
                    .unwrap_or_default()
            };
            stats::spawn_episode_end(&mut commands, &ui, name, difficulty.0, &totals, client);
        }
        Some(other) => multiplayer::spawn(&mut commands, &ui, other, &net),
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
        assert_eq!(
            Screen::Difficulty.back(PlayState::Menu),
            Some(Screen::Episodes)
        );
        assert_eq!(Screen::Episodes.back(PlayState::Menu), Some(Screen::Main));
        assert_eq!(Screen::Options.back(PlayState::Menu), Some(Screen::Main));
        assert_eq!(Screen::Options.back(PlayState::Paused), Some(Screen::Pause));
        assert_eq!(
            Screen::Controls.back(PlayState::Paused),
            Some(Screen::Options)
        );
        assert_eq!(Screen::Pause.back(PlayState::Paused), None);
        assert_eq!(
            Screen::Multiplayer.back(PlayState::Menu),
            Some(Screen::Main)
        );
        assert_eq!(
            Screen::Join.back(PlayState::Menu),
            Some(Screen::Multiplayer)
        );
        assert_eq!(Screen::Lobby.back(PlayState::Menu), None);
        assert_eq!(Screen::NetNotice.back(PlayState::Menu), None);
        assert_eq!(
            Screen::Episodes.back_from(PlayState::Menu, true),
            Some(Screen::Multiplayer)
        );
        assert_eq!(
            Screen::Episodes.back_from(PlayState::Menu, false),
            Some(Screen::Main)
        );
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
        assert_eq!(
            Screen::for_state(EpisodeEnd, false),
            Some(Screen::EpisodeEnd)
        );
    }

    #[test]
    fn a_client_follows_the_hosts_run() {
        assert!(!stats_screens(
            NetMode::Client,
            true,
            Some(&HostRun::default())
        ));
        let run = HostRun {
            episode: true,
            ..default()
        };
        assert!(stats_screens(NetMode::Client, false, Some(&run)));
        assert!(stats_screens(NetMode::Host, true, None));
        assert!(stats_screens(NetMode::Single, true, None));
        assert!(!stats_screens(NetMode::Single, false, Some(&run)));
    }
}
