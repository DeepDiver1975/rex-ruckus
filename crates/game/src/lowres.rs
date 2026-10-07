//! Low-res mode: the 3D view (viewmodel included) renders into a 360-row image, as wide as the
//! window's aspect needs so pixels stay square, shown full screen behind the UI, so the HUD and
//! menus stay crisp at window resolution.

use crate::player::PlayerCamera;
use crate::settings::Settings;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::{PrimaryWindow, WindowRef, WindowResized};

/// Rows of the low-res image; its width follows the window so pixels stay square.
pub const LOW_RES_ROWS: u32 = 360;

/// Size of the low-res image for a window of `window` physical pixels: 360 rows, an even width
/// matching the aspect (clamped to 2..=4096). A zero-sized (minimised) window gives 640x360.
pub fn low_res_size(window: UVec2) -> UVec2 {
    if window.x == 0 || window.y == 0 {
        return UVec2::new(640, LOW_RES_ROWS);
    }
    let w = (LOW_RES_ROWS as f32 * window.x as f32 / window.y as f32).round() as u32;
    UVec2::new(((w + 1) & !1).clamp(2, 4096), LOW_RES_ROWS)
}

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
        app.add_systems(Startup, setup_low_res).add_systems(
            Update,
            (
                apply_low_res.run_if(resource_changed::<Settings>),
                resize_low_res,
            ),
        );
    }
}

fn setup_low_res(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let size = low_res_size(window.single().map_or(UVec2::ZERO, |w| w.physical_size()));
    let image = Image::new_target_texture(
        size.x,
        size.y,
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

/// Keeps the low-res image at the window's aspect when the window is resized.
fn resize_low_res(
    mut resized: MessageReader<WindowResized>,
    window: Query<&Window, With<PrimaryWindow>>,
    image: Option<Res<LowResImage>>,
    mut images: ResMut<Assets<Image>>,
) {
    if resized.read().count() == 0 {
        return;
    }
    let (Some(image), Ok(window)) = (image, window.single()) else {
        return;
    };
    let size = low_res_size(window.physical_size());
    let Some(img) = images.get(&image.0) else {
        return;
    };
    if img.size() == size {
        return;
    }
    if let Some(mut img) = images.get_mut(&image.0) {
        img.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
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
    fn width_follows_the_window_aspect() {
        assert_eq!(low_res_size(UVec2::new(1280, 720)), UVec2::new(640, 360));
        assert_eq!(low_res_size(UVec2::new(1024, 768)), UVec2::new(480, 360));
        assert_eq!(low_res_size(UVec2::new(2560, 1080)), UVec2::new(854, 360));
    }

    #[test]
    fn degenerate_windows_stay_valid() {
        assert_eq!(low_res_size(UVec2::ZERO), UVec2::new(640, 360));
        assert_eq!(low_res_size(UVec2::new(1, 4000)).x, 2);
        assert_eq!(low_res_size(UVec2::new(100_000, 10)).x, 4096);
    }

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
