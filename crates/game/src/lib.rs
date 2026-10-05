//! Bevy front-end for Rex Ruckus: Meltdown.

pub mod combat;
pub mod coords;
pub mod hud;
pub mod level;
pub mod mechanics;
pub mod paths;
pub mod player;
pub mod props;
pub mod textures;

use bevy::prelude::*;
use level::{LevelRenderPlugin, load_map};

pub struct GamePlugin {
    /// Level file name inside `assets/levels/`.
    pub level: String,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        mechanics::insert_level(app, load_map(&self.level));
        combat::insert_defs(app, combat::load_defs());
        app.add_plugins((
            LevelRenderPlugin,
            player::PlayerSimPlugin,
            mechanics::MechanicsSimPlugin,
            combat::CombatSimPlugin,
            player::PlayerControlPlugin,
            props::PropsPlugin,
            hud::HudPlugin,
        ));
    }
}
