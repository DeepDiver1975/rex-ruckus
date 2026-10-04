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
        .add_systems(Startup, |mut commands: Commands| {
            // Temporary overview camera; replaced by the player camera in Task 8.
            commands.spawn((
                Camera3d::default(),
                Transform::from_xyz(8.0, 25.0, 10.0)
                    .looking_at(Vec3::new(10.0, 0.0, -10.0), Vec3::Y),
            ));
        })
        .run();
}
