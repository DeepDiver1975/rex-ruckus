use bevy::prelude::*;
use rr_game::GamePlugin;

fn main() {
    let level = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mechanics_lab.ron".into());
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
        .add_plugins(GamePlugin { level })
        .run();
}
