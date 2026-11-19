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
/// A co-op player's row and the client's waiting note.
const PLAYER_PX: f32 = 26.0;

/// The kills, secrets and time rows of a stats screen.
pub fn stats_lines(s: &LevelStats) -> [String; 3] {
    [
        format!("Kills  {} / {}", s.kills, s.kills_total),
        format!("Secrets  {} / {}", s.secrets, s.secrets_total),
        format!("Time  {}", format_time(s.seconds(TICK_HZ))),
    ]
}

/// One row per co-op player, `names[i]` with `kills_by[i]`; none in single play (no names).
pub fn player_lines(s: &LevelStats, names: &[String]) -> Vec<String> {
    names
        .iter()
        .zip(s.kills_by)
        .map(|(name, k)| format!("{name}  {k}"))
        .collect()
}

/// "LEVEL COMPLETE": the level's name and tallies, each co-op player's kills (`names`, index =
/// `PlayerId`) and a Continue button, or, on a co-op client (`can_continue` false), a note
/// that the host moves on.
pub fn spawn_stats(
    commands: &mut Commands,
    ui: &UiFont,
    level: &str,
    stats: &LevelStats,
    names: &[String],
    can_continue: bool,
) {
    let root = spawn_screen(commands, ui, "LEVEL COMPLETE");
    commands.entity(root).with_children(|p| {
        label(p, ui, level, 36.0, AMBER);
        for line in stats_lines(stats) {
            label(p, ui, &line, BODY_PX, Color::WHITE);
        }
        for line in player_lines(stats, names) {
            label(p, ui, &line, PLAYER_PX, Color::WHITE);
        }
        if can_continue {
            button(p, ui, "Continue", MenuAction::Continue);
        } else {
            label(p, ui, "Waiting for the host", PLAYER_PX, AMBER);
        }
    });
}

/// "EPISODE COMPLETE": the episode name, the difficulty, the episode totals and Rex's closing
/// line; a co-op client (`client`) leaves the game from here instead of going to the menu.
pub fn spawn_episode_end(
    commands: &mut Commands,
    ui: &UiFont,
    episode: &str,
    difficulty: Difficulty,
    totals: &LevelStats,
    client: bool,
) {
    let root = spawn_screen(commands, ui, "EPISODE COMPLETE");
    commands.entity(root).with_children(|p| {
        if !episode.is_empty() {
            label(p, ui, episode, 36.0, AMBER);
        }
        label(p, ui, difficulty.label(), 36.0, AMBER);
        for line in stats_lines(totals) {
            label(p, ui, &line, BODY_PX, Color::WHITE);
        }
        label(p, ui, CLOSING_LINE, 24.0, AMBER);
        if client {
            button(p, ui, "Leave game", MenuAction::LeaveGame);
        } else {
            button(p, ui, "Main menu", MenuAction::QuitToMenu);
        }
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
            ..Default::default()
        };
        assert_eq!(
            stats_lines(&s),
            ["Kills  3 / 5", "Secrets  1 / 1", "Time  2:05"]
        );
    }

    #[test]
    fn player_rows_pair_names_with_kills() {
        let s = LevelStats {
            kills_by: [4, 0, 2, 9],
            ..Default::default()
        };
        assert!(player_lines(&s, &[]).is_empty());
        let names = ["Rex".to_string(), "Ana".into(), "Bo".into()];
        assert_eq!(player_lines(&s, &names), ["Rex  4", "Ana  0", "Bo  2"]);
    }
}
