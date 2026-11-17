//! The main menu, the episode picker and the difficulty picker.

use super::MenuAction;
use super::widgets::{AMBER, DIM, button, label, spawn_screen, spawn_screen_sized};
use crate::episode::EpisodeDef;
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
        button(p, ui, "Multiplayer", MenuAction::Multiplayer);
        button(p, ui, "Options", MenuAction::Options);
        button(p, ui, "Quit", MenuAction::QuitGame);
    });
}

/// Episode button label: its number and name, e.g. "Episode 2: Aftershock".
pub fn episode_label(i: usize, def: &EpisodeDef) -> String {
    let soon = if def.playable() { "" } else { " (coming soon)" };
    format!("Episode {}: {}{soon}", i + 1, def.name)
}

/// The episode picker shown after New Game: a button per playable episode, a dimmed line for
/// one without levels yet.
pub fn spawn_episodes(commands: &mut Commands, ui: &UiFont, choices: &[EpisodeDef]) {
    let root = spawn_screen(commands, ui, "Choose your episode");
    commands.entity(root).with_children(|p| {
        for (i, def) in choices.iter().enumerate() {
            let text = episode_label(i, def);
            if def.playable() {
                button(p, ui, &text, MenuAction::PickEpisode(i));
            } else {
                label(p, ui, &text, 26.0, DIM);
            }
        }
        button(p, ui, "Back", MenuAction::Back);
    });
}

/// The skill picker shown after the episode picker.
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
    fn episode_labels_number_and_flag_unplayable() {
        let mut def = EpisodeDef {
            name: "Aftershock".into(),
            levels: vec![],
        };
        assert_eq!(
            episode_label(1, &def),
            "Episode 2: Aftershock (coming soon)"
        );
        def.levels.push("a.ron".into());
        assert_eq!(episode_label(1, &def), "Episode 2: Aftershock");
    }

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
