//! Low-res mode: the 3D view (viewmodel included) renders into a 640x360 image that is shown
//! full screen behind the UI, so the HUD and menus stay crisp at window resolution.

use crate::player::PlayerCamera;
use crate::settings::Settings;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::WindowRef;

/// Render resolution of the low-res mode.
pub const LOW_RES: UVec2 = UVec2::new(640, 360);

/// The image the 3D camera renders into while low-res is on.
#[derive(Resource)]
pub struct LowResImage(pub Handle<Image>);

/// The full-screen node that shows [`LowResImage`].
#[derive(Component)]
pub struct LowResView;

/// Where the 3D camera renders: the window, or `image` in low-res mode.
pub fn target_for(low_res: bool, image: &Handle<Image>) -> RenderTarget {
    if low_res {
        RenderTarget::Image(image.clone().into())
    } else {
        RenderTarget::Window(WindowRef::Primary)
    }
}

pub struct LowResPlugin;

impl Plugin for LowResPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_low_res)
            .add_systems(Update, apply_low_res.run_if(resource_changed::<Settings>));
    }
}

fn setup_low_res(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = Image::new_target_texture(
        LOW_RES.x,
        LOW_RES.y,
        TextureFormat::Bgra8Unorm,
        Some(TextureFormat::Bgra8UnormSrgb),
    );
    let handle = images.add(image);
    commands.insert_resource(LowResImage(handle.clone()));
    // The UI always renders through this camera, at window resolution, over the 3D view.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
    ));
    commands.spawn((
        ImageNode::new(handle),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(-10),
        Visibility::Hidden,
        LowResView,
    ));
}

fn apply_low_res(
    settings: Res<Settings>,
    image: Option<Res<LowResImage>>,
    mut commands: Commands,
    camera: Query<Entity, With<PlayerCamera>>,
    mut view: Query<&mut Visibility, With<LowResView>>,
) {
    let Some(image) = image else { return };
    for entity in &camera {
        commands
            .entity(entity)
            .insert(target_for(settings.low_res, &image.0));
    }
    for mut v in &mut view {
        *v = if settings.low_res {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_follows_the_toggle() {
        let h = Handle::<Image>::default();
        assert!(matches!(
            target_for(false, &h),
            RenderTarget::Window(WindowRef::Primary)
        ));
        match target_for(true, &h) {
            RenderTarget::Image(t) => assert_eq!(t.handle, h),
            _ => panic!("expected an image target"),
        }
    }
}
