//! The pause menu.

use super::MenuAction;
use super::widgets::{button, spawn_screen};
use crate::hud::UiFont;
use bevy::prelude::*;

/// "Quit to menu" needs an episode to go back to, so direct-level runs leave it out.
pub fn spawn_pause(commands: &mut Commands, ui: &UiFont, has_episode: bool, can_resume: bool) {
    let root = spawn_screen(commands, ui, "PAUSED");
    commands.entity(root).with_children(|p| {
        // Resuming a corpse makes no sense: from the death screen only restart or quit.
        if can_resume {
            button(p, ui, "Resume", MenuAction::Resume);
        }
        button(p, ui, "Options", MenuAction::Options);
        button(p, ui, "Restart level", MenuAction::RestartLevel);
        if has_episode {
            button(p, ui, "Quit to menu", MenuAction::QuitToMenu);
        }
        button(p, ui, "Quit game", MenuAction::QuitGame);
    });
}
