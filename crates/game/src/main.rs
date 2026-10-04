use bevy::prelude::*;
use rr_game::GamePlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Rex Ruckus: Meltdown".into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin {
            level: "test_yard.ron".into(),
        })
        .run();
}
