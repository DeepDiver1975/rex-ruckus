//! The pause menu.

use super::MenuAction;
use super::widgets::{button, spawn_screen};
use crate::hud::UiFont;
use crate::net::NetMode;
use bevy::prelude::*;

/// "Quit to menu" needs an episode to go back to, so direct-level runs leave it out. In a net game
/// it is "Leave game" instead, and only the host may restart the level.
pub fn spawn_pause(
    commands: &mut Commands,
    ui: &UiFont,
    has_episode: bool,
    can_resume: bool,
    net: NetMode,
) {
    let root = spawn_screen(commands, ui, "PAUSED");
    commands.entity(root).with_children(|p| {
        // Resuming a corpse makes no sense: from the death screen only restart or quit.
        if can_resume {
            button(p, ui, "Resume", MenuAction::Resume);
        }
        button(p, ui, "Options", MenuAction::Options);
        if net != NetMode::Client {
            button(p, ui, "Restart level", MenuAction::RestartLevel);
        }
        if net.is_net() {
            button(p, ui, "Leave game", MenuAction::LeaveGame);
        } else if has_episode {
            button(p, ui, "Quit to menu", MenuAction::QuitToMenu);
        }
        button(p, ui, "Quit game", MenuAction::QuitGame);
    });
}
