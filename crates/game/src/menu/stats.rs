//! The end-of-level stats screen and the episode-end screen.

use super::MenuAction;
use super::widgets::{AMBER, button, label, spawn_screen};
use crate::hud::UiFont;
use bevy::prelude::*;
use rr_core::difficulty::Difficulty;
use rr_core::stats::{LevelStats, format_time};

/// Rex's closing line on the episode-end screen.
const CLOSING_LINE: &str = "Meltdown averted. Somebody owes me a cold one.";
/// Fixed ticks per second of the simulation.
const TICK_HZ: f32 = 60.0;
const BODY_PX: f32 = 32.0;

/// The kills, secrets and time rows of a stats screen.
pub fn stats_lines(s: &LevelStats) -> [String; 3] {
    [
        format!("Kills  {} / {}", s.kills, s.kills_total),
        format!("Secrets  {} / {}", s.secrets, s.secrets_total),
        format!("Time  {}", format_time(s.seconds(TICK_HZ))),
    ]
}

/// "LEVEL COMPLETE": the level's name and tallies, and a Continue button.
pub fn spawn_stats(commands: &mut Commands, ui: &UiFont, level: &str, stats: &LevelStats) {
    let root = spawn_screen(commands, ui, "LEVEL COMPLETE");
    commands.entity(root).with_children(|p| {
        label(p, ui, level, 36.0, AMBER);
        for line in stats_lines(stats) {
            label(p, ui, &line, BODY_PX, Color::WHITE);
        }
        button(p, ui, "Continue", MenuAction::Continue);
    });
}

/// "EPISODE COMPLETE": the difficulty, the episode totals and Rex's closing line.
pub fn spawn_episode_end(
    commands: &mut Commands,
    ui: &UiFont,
    difficulty: Difficulty,
    totals: &LevelStats,
) {
    let root = spawn_screen(commands, ui, "EPISODE COMPLETE");
    commands.entity(root).with_children(|p| {
        label(p, ui, difficulty.label(), 36.0, AMBER);
        for line in stats_lines(totals) {
            label(p, ui, &line, BODY_PX, Color::WHITE);
        }
        label(p, ui, CLOSING_LINE, 24.0, AMBER);
        button(p, ui, "Main menu", MenuAction::QuitToMenu);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_show_kills_secrets_and_time() {
        let s = LevelStats {
            kills: 3,
            kills_total: 5,
            secrets: 1,
            secrets_total: 1,
            ticks: 7524,
        };
        assert_eq!(
            stats_lines(&s),
            ["Kills  3 / 5", "Secrets  1 / 1", "Time  2:05"]
        );
    }
}
