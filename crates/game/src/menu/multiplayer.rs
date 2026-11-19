//! The co-op screens: Multiplayer (name, host, join), Join (LAN games and an address), the Lobby
//! and the NetNotice screen. Their text fields live in `text_field`.
//!
//! Hosting goes through the Episodes and Difficulty screens with [`HostSetup`] set; the chosen
//! difficulty then opens the game and shows the Lobby instead of starting the episode.

use std::net::{Ipv4Addr, SocketAddr};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_net::{GAME_PORT, parse_addr};

use super::main_menu::difficulty_label;
use super::text_field::{Field, FieldText};
pub use super::text_field::{JoinAddress, NameOverride};
use super::widgets::{AMBER, DIM, button, button_marked, label, spawn_screen};
use super::{MenuAction, MenuScreen, Screen};
use crate::episode::{Episode, start_episode};
use crate::flow::{LevelDifficulty, LevelFile, PlayState};
use crate::hud::{UiFont, font};
use crate::net::client::{HostRun, NetClient, NetNotice, start_joining, stop_joining};
use crate::net::host::{NetHost, NetIdentity, start_hosting, stop_hosting, unhostable_level};
use crate::net::lobby::LanGames;
use crate::net::{NetMode, NetStart};
use crate::settings::{Settings, default_player_name};

/// Set by "Host game": the difficulty picker hosts instead of starting the episode.
#[derive(Resource, Default, Debug)]
pub struct HostSetup(pub bool);

/// The port a game is hosted on ([`GAME_PORT`]; tests use 0 for a free one).
#[derive(Resource, Debug)]
pub struct HostPort(pub u16);

impl Default for HostPort {
    fn default() -> Self {
        Self(GAME_PORT)
    }
}

/// The Lobby's level, skill and player lines.
#[derive(Component)]
pub struct LobbyText;

/// The label of the Connect button (dimmed while the address does not parse).
#[derive(Component)]
pub struct ConnectText;

/// The name this process plays under: `--name`, else the saved one.
pub fn player_name(world: &World) -> String {
    if let Some(n) = world.get_resource::<NameOverride>() {
        return n.0.clone();
    }
    world
        .get_resource::<Settings>()
        .map_or_else(default_player_name, |s| s.player_name.clone())
}

/// Every player's name in seat order: the host's view, else the client's (empty when neither).
pub fn roster_names(
    host: Option<&NetHost>,
    identity: Option<&NetIdentity>,
    client: Option<&NetClient>,
) -> Vec<String> {
    if let (Some(host), Some(id)) = (host, identity) {
        std::iter::once(id.name.clone())
            .chain(host.seats.iter().map(|s| s.name.clone()))
            .collect()
    } else {
        client.map(|c| c.roster.clone()).unwrap_or_default()
    }
}

/// What the co-op screens show, for `rebuild_screen`.
#[derive(SystemParam)]
pub struct NetView<'w> {
    mode: Option<Res<'w, NetMode>>,
    lan: Option<Res<'w, LanGames>>,
    notice: Option<Res<'w, NetNotice>>,
    host: Option<Res<'w, NetHost>>,
    identity: Option<Res<'w, NetIdentity>>,
    client: Option<Res<'w, NetClient>>,
    run: Option<Res<'w, HostRun>>,
}

impl NetView<'_> {
    pub fn mode(&self) -> NetMode {
        self.mode.as_deref().copied().unwrap_or_default()
    }

    /// Every player's name in a net game (`net::player_names`), empty in single play.
    pub fn names(&self) -> Vec<String> {
        if !self.mode().is_net() {
            return Vec::new();
        }
        roster_names(
            self.host.as_deref(),
            self.identity.as_deref(),
            self.client.as_deref(),
        )
    }

    /// What a client knows of the host's run.
    pub fn run(&self) -> Option<&HostRun> {
        self.run.as_deref()
    }
}

/// Spawns a co-op screen (nothing for any other screen).
pub fn spawn(commands: &mut Commands, ui: &UiFont, screen: Screen, view: &NetView) {
    match screen {
        Screen::Multiplayer => spawn_multiplayer(commands, ui),
        Screen::Join => spawn_join(commands, ui, view.lan.as_deref()),
        Screen::Lobby => spawn_lobby(commands, ui, view.mode() == NetMode::Host),
        Screen::NetNotice => {
            let text = view.notice.as_ref().and_then(|n| n.0.clone());
            spawn_notice(commands, ui, &text.unwrap_or_default());
        }
        _ => {}
    }
}

fn spawn_multiplayer(commands: &mut Commands, ui: &UiFont) {
    let root = spawn_screen(commands, ui, "MULTIPLAYER");
    commands.entity(root).with_children(|p| {
        button_marked(
            p,
            ui,
            "",
            MenuAction::Edit(Field::Name),
            FieldText(Field::Name),
        );
        button(p, ui, "Host game", MenuAction::HostGame);
        button(p, ui, "Join game", MenuAction::JoinGame);
        button(p, ui, "Back", MenuAction::Back);
    });
}

fn spawn_join(commands: &mut Commands, ui: &UiFont, lan: Option<&LanGames>) {
    let root = spawn_screen(commands, ui, "JOIN GAME");
    commands.entity(root).with_children(|p| {
        match lan {
            Some(lan) if lan.unavailable => label(p, ui, "LAN search unavailable", 22.0, DIM),
            Some(lan) if !lan.games.is_empty() => {
                for game in &lan.games {
                    button(p, ui, &game.label(), MenuAction::JoinLan(game.addr));
                }
            }
            _ => label(p, ui, "Looking for LAN games…", 22.0, DIM),
        }
        button_marked(
            p,
            ui,
            "",
            MenuAction::Edit(Field::Address),
            FieldText(Field::Address),
        );
        button_marked(p, ui, "Connect", MenuAction::Connect, ConnectText);
        button(p, ui, "Back", MenuAction::Back);
    });
}

fn spawn_lobby(commands: &mut Commands, ui: &UiFont, host: bool) {
    let root = spawn_screen(commands, ui, "LOBBY");
    commands.entity(root).with_children(|p| {
        p.spawn((
            Text::new(""),
            font(ui, 24.0),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
            LobbyText,
        ));
        if host {
            button(p, ui, "Start", MenuAction::StartGame);
        }
        button(p, ui, "Leave", MenuAction::Leave);
    });
}

fn spawn_notice(commands: &mut Commands, ui: &UiFont, text: &str) {
    let root = spawn_screen(commands, ui, "MULTIPLAYER");
    commands.entity(root).with_children(|p| {
        label(p, ui, text, 26.0, AMBER);
        button(p, ui, "OK", MenuAction::NoticeOk);
    });
}

/// The Lobby text: the level, the skill and the players, or "Connecting…" before the welcome.
fn lobby_text(level: &str, skill: Difficulty, names: &[String]) -> String {
    let mut lines = vec![
        format!("Level: {level}"),
        format!("Skill: {}", difficulty_label(skill)),
        format!("Players ({}):", names.len()),
    ];
    lines.extend(names.iter().cloned());
    lines.join("\n")
}

/// The data the Lobby and Connect texts show.
#[derive(SystemParam)]
pub struct LobbySources<'w> {
    address: Res<'w, JoinAddress>,
    host: Option<Res<'w, NetHost>>,
    identity: Option<Res<'w, NetIdentity>>,
    client: Option<Res<'w, NetClient>>,
    file: Option<Res<'w, LevelFile>>,
    skill: Option<Res<'w, LevelDifficulty>>,
}

/// Keeps the Lobby and Connect texts current.
pub fn refresh_texts(
    src: LobbySources,
    mut lobby: Query<&mut Text, With<LobbyText>>,
    mut connect: Query<&mut TextColor, With<ConnectText>>,
) {
    for mut text in &mut lobby {
        let names = roster_names(
            src.host.as_deref(),
            src.identity.as_deref(),
            src.client.as_deref(),
        );
        let wanted = match (&src.client, &src.host) {
            (Some(client), _) => match &client.welcomed {
                Some(w) => lobby_text(&w.level, w.skill, &names),
                None => "Connecting…".into(),
            },
            (None, Some(_)) => lobby_text(
                src.file.as_ref().map_or("", |f| f.0.as_str()),
                src.skill.as_ref().map(|s| s.0).unwrap_or_default(),
                &names,
            ),
            (None, None) => String::new(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let usable = parse_addr(&src.address.0).is_some();
    for mut color in &mut connect {
        let wanted = TextColor(if usable { Color::WHITE } else { DIM });
        if *color != wanted {
            *color = wanted;
        }
    }
}

/// Shows the NetNotice screen while a notice is set.
pub fn show_notice(notice: Option<Res<NetNotice>>, mut screen: ResMut<MenuScreen>) {
    if notice.is_some_and(|n| n.0.is_some()) && screen.0 != Some(Screen::NetNotice) {
        screen.0 = Some(Screen::NetNotice);
    }
}

fn set_screen(world: &mut World, s: Screen) {
    if let Some(mut screen) = world.get_resource_mut::<MenuScreen>() {
        screen.0 = Some(s);
    }
}

fn notice(world: &mut World, text: String) {
    warn!("{text}");
    world.insert_resource(NetNotice(Some(text)));
}

/// Opens the hosted game on the chosen difficulty and shows the Lobby (or the notice).
pub fn host_game(world: &mut World, d: Difficulty) {
    world.resource_mut::<HostSetup>().0 = false;
    world.resource_mut::<LevelDifficulty>().0 = d;
    if let Some(file) = world.get_resource::<Episode>().map(|e| e.file(0)) {
        world.insert_resource(LevelFile(file));
    }
    if open_host(world) {
        set_screen(world, Screen::Lobby);
    }
}

/// Hosts on [`HostPort`]; false (with a notice) if the level cannot be hosted or the port
/// cannot be opened.
fn open_host(world: &mut World) -> bool {
    if let Some(why) = unhostable_level(world) {
        notice(world, why);
        return false;
    }
    let port = world.get_resource::<HostPort>().map_or(GAME_PORT, |p| p.0);
    let name = player_name(world);
    match start_hosting(world, SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)), name) {
        Ok(()) => true,
        Err(e) => {
            notice(world, format!("Could not open port {port}: {e}"));
            false
        }
    }
}

/// Joins the game at `addr` and shows the Lobby (or the notice).
pub fn join(world: &mut World, addr: SocketAddr) {
    let name = player_name(world);
    match start_joining(world, addr, name) {
        Ok(()) => set_screen(world, Screen::Lobby),
        Err(e) => notice(world, format!("Could not join {addr}: {e}")),
    }
}

/// Joins the address typed in the field, if it parses.
pub fn connect(world: &mut World) {
    if let Some(addr) = parse_addr(&world.resource::<JoinAddress>().0) {
        join(world, addr);
    }
}

/// The host's Start: the episode begins on the chosen skill; the new level run sends `Load`.
pub fn start_game(world: &mut World) {
    let hosting = world.get_resource::<NetMode>() == Some(&NetMode::Host);
    if hosting && world.contains_resource::<Episode>() {
        let d = world.resource::<LevelDifficulty>().0;
        start_episode(world, d);
    }
}

/// Ends the co-op session, if any.
pub fn stop_session(world: &mut World) {
    match world.get_resource::<NetMode>().copied().unwrap_or_default() {
        NetMode::Host => stop_hosting(world),
        NetMode::Client => stop_joining(world),
        NetMode::Single => {}
    }
}

/// The Lobby's Leave: back to the Multiplayer screen.
pub fn leave_lobby(world: &mut World) {
    stop_session(world);
    set_screen(world, Screen::Multiplayer);
}

/// The notice's OK: clears it and shows the play state's screen (the title in the menu).
pub fn notice_ok(world: &mut World) {
    world.insert_resource(NetNotice(None));
    let state = *world.resource::<PlayState>();
    let wanted = Screen::for_state(state, super::stats_screens_in(world));
    world.resource_mut::<MenuScreen>().0 = wanted;
}

/// `--host` / `--join`: opens the session at startup. A host that cannot open its port is left
/// at the title screen with the notice (in an episode run).
pub fn start_from_command_line(world: &mut World, start: NetStart) {
    match start {
        NetStart::Host => {
            if !open_host(world) && world.contains_resource::<Episode>() {
                *world.resource_mut::<PlayState>() = PlayState::Menu;
            }
        }
        NetStart::Join(addr) => join(world, addr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_text_lists_level_skill_and_players() {
        let names = ["Ann".to_string(), "Bo".to_string()];
        assert_eq!(
            lobby_text("rubble_run.ron", Difficulty::Hard, &names),
            "Level: rubble_run.ron\nSkill: Hard: All-you-can-eat carnage\nPlayers (2):\nAnn\nBo"
        );
    }
}
