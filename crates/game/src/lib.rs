//! Bevy front-end for Rex Ruckus: Meltdown.

pub mod actors;
pub mod audio;
pub mod bindings;
pub mod breakables;
pub mod combat;
pub mod coords;
pub mod decals;
pub mod demo;
pub mod episode;
pub mod flow;
pub mod fx;
pub mod hud;
pub mod inventory;
pub mod level;
pub mod mechanics;
pub mod menu;
pub mod models;
pub mod paths;
pub mod player;
pub mod props;
pub mod settings;
pub mod textures;
pub mod viewmodel;

use bevy::prelude::*;
use level::{LevelRenderPlugin, load_map};

pub struct GamePlugin {
    /// Level file name inside `assets/levels/`; ignored when `episode` is set.
    pub level: String,
    /// Play this episode from its first level (the level and the carried loadout then follow the
    /// episode); `None` for a direct-level or demo run.
    pub episode: Option<episode::EpisodeDef>,
    /// Replay this demo script instead of reading the keyboard and mouse.
    pub demo: Option<demo::DemoPlugin>,
    /// Start muted, play music (see `audio::AudioOptions`).
    pub audio: audio::AudioOptions,
    /// Skill the level is played on.
    pub difficulty: rr_core::difficulty::Difficulty,
    /// Settings file to read and save; `None` uses defaults and never touches disk.
    pub settings: Option<std::path::PathBuf>,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        let first = match &self.episode {
            Some(def) => {
                let episode = episode::Episode::new(def.clone());
                let first = episode.map(0);
                app.insert_resource(episode);
                first
            }
            None => load_map(&self.level),
        };
        mechanics::insert_level(app, first, self.difficulty);
        combat::insert_defs(app, combat::load_defs());
        // Models load before the plugins whose spawns use them.
        app.add_plugins(models::ModelsPlugin);
        // Before the plugins that only insert look and binding defaults when absent.
        app.add_plugins(settings::SettingsPlugin {
            path: self.settings.clone(),
        });
        app.add_plugins((
            flow::FlowPlugin,
            LevelRenderPlugin,
            player::PlayerSimPlugin,
            mechanics::MechanicsSimPlugin,
            combat::CombatSimPlugin,
            player::PlayerControlPlugin {
                scripted: self.demo.is_some(),
            },
            props::PropsPlugin,
            actors::ActorVisualsPlugin,
            fx::FxPlugin,
            decals::DecalsPlugin,
            breakables::BreakablesPlugin,
            inventory::NightVisionPlugin,
            hud::HudPlugin,
            viewmodel::ViewModelPlugin,
            audio::AudioFxPlugin {
                options: self.audio,
                scripted: self.demo.is_some(),
            },
        ));
        app.add_plugins(episode::EpisodePlugin);
        // The episode starts at the title screen; direct-level and demo runs go straight to play.
        if self.episode.is_some() && self.demo.is_none() {
            app.insert_resource(flow::PlayState::Menu);
        }
        app.add_plugins(menu::MenuPlugin {
            enabled: self.demo.is_none(),
        });
        if let Some(demo) = &self.demo {
            app.add_plugins(demo::DemoPlugin {
                script: demo.script.clone(),
                record: demo.record.clone(),
            });
        }
    }
}
