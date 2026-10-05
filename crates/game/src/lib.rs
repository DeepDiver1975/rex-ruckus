//! Bevy front-end for Rex Ruckus: Meltdown.

pub mod actors;
pub mod audio;
pub mod breakables;
pub mod combat;
pub mod coords;
pub mod decals;
pub mod demo;
pub mod flow;
pub mod fx;
pub mod hud;
pub mod inventory;
pub mod level;
pub mod mechanics;
pub mod paths;
pub mod player;
pub mod props;
pub mod textures;
pub mod viewmodel;

use bevy::prelude::*;
use level::{LevelRenderPlugin, load_map};

pub struct GamePlugin {
    /// Level file name inside `assets/levels/`.
    pub level: String,
    /// Replay this demo script instead of reading the keyboard and mouse.
    pub demo: Option<demo::DemoPlugin>,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        mechanics::insert_level(app, load_map(&self.level));
        combat::insert_defs(app, combat::load_defs());
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
            audio::AudioFxPlugin,
        ));
        if let Some(demo) = &self.demo {
            app.add_plugins(demo::DemoPlugin {
                script: demo.script.clone(),
                record: demo.record.clone(),
            });
        }
    }
}
