//! The window and taskbar icon. Bevy has no `Window` field for it, so this sets it on the winit
//! window once that exists. The exe's own icon (Explorer, shortcuts) is embedded by `build.rs`.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

/// Built by `scripts/build-icons.sh` from `assets/icon/rex-ruckus.svg`.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/rex-ruckus-256.png");

/// Sets the primary window's icon on the first frame it exists.
pub struct IconPlugin;

impl Plugin for IconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, set_window_icon.run_if(not(icon_set)));
    }
}

/// Marks the icon as set (or failed), so the system stops running.
#[derive(Resource)]
struct IconSet;

fn icon_set(done: Option<Res<IconSet>>) -> bool {
    done.is_some()
}

/// Decodes the embedded PNG into a winit icon.
pub fn window_icon() -> Result<winit::window::Icon, String> {
    let rgba = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| format!("icon PNG: {e}"))?
        .into_rgba8();
    let (w, h) = rgba.dimensions();
    winit::window::Icon::from_rgba(rgba.into_raw(), w, h).map_err(|e| format!("icon: {e}"))
}

fn set_window_icon(
    mut commands: Commands,
    window: Query<Entity, With<PrimaryWindow>>,
    _main_thread: NonSendMarker,
) {
    let Ok(entity) = window.single() else {
        return;
    };
    let set = WINIT_WINDOWS.with_borrow(|windows| {
        let Some(winit_window) = windows.get_window(entity) else {
            return false;
        };
        match window_icon() {
            Ok(icon) => winit_window.set_window_icon(Some(icon)),
            Err(e) => warn!("window icon not set: {e}"),
        }
        true
    });
    if set {
        commands.insert_resource(IconSet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes() {
        assert!(window_icon().is_ok());
        let img = image::load_from_memory(ICON_PNG).unwrap();
        assert_eq!((img.width(), img.height()), (256, 256));
    }
}
