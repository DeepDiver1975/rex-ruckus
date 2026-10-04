//! Bevy front-end for Rex Ruckus: Meltdown.

pub mod coords;
pub mod level;
pub mod paths;
pub mod textures;

use bevy::prelude::*;
use level::{CurrentMap, LevelRenderPlugin, load_map};

pub struct GamePlugin {
    /// Level file name inside `assets/levels/`.
    pub level: String,
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CurrentMap(load_map(&self.level)))
            .add_plugins(LevelRenderPlugin);
    }
}
