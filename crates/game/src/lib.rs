//! Bevy front-end for Rex Ruckus: Meltdown.

pub mod coords;
pub mod level;
pub mod mechanics;
pub mod paths;
pub mod player;
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
        app.add_plugins((
            LevelRenderPlugin,
            player::PlayerSimPlugin,
            mechanics::MechanicsSimPlugin,
            player::PlayerControlPlugin,
        ));
    }
}
