//! The main menu and the difficulty picker.

use super::MenuAction;
use super::widgets::{AMBER, button, label, spawn_screen, spawn_screen_sized};
use crate::hud::UiFont;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;

/// Difficulty button labels: the skill and its (original) tagline.
pub fn difficulty_label(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Easy => "Easy: Light snack",
        Difficulty::Normal => "Normal: Full meal",
        Difficulty::Hard => "Hard: All-you-can-eat carnage",
    }
}

/// The title screen.
pub fn spawn_main(commands: &mut Commands, ui: &UiFont) {
    let root = spawn_screen_sized(commands, ui, "REX RUCKUS", 64.0);
    commands.entity(root).with_children(|p| {
        label(p, ui, "MELTDOWN", 36.0, AMBER);
        button(p, ui, "New Game", MenuAction::NewGame);
        button(p, ui, "Options", MenuAction::Options);
        button(p, ui, "Quit", MenuAction::QuitGame);
    });
}

/// The skill picker shown after New Game.
pub fn spawn_difficulty(commands: &mut Commands, ui: &UiFont) {
    let root = spawn_screen(commands, ui, "Choose your poison");
    commands.entity(root).with_children(|p| {
        for d in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            button(p, ui, difficulty_label(d), MenuAction::PickDifficulty(d));
        }
        button(p, ui, "Back", MenuAction::Back);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taglines_are_the_original_ones() {
        assert_eq!(difficulty_label(Difficulty::Easy), "Easy: Light snack");
        assert_eq!(difficulty_label(Difficulty::Normal), "Normal: Full meal");
        assert_eq!(
            difficulty_label(Difficulty::Hard),
            "Hard: All-you-can-eat carnage"
        );
    }
}
