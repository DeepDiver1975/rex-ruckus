//! Player settings: mouse, field of view, volumes, low-res and key bindings, saved as RON in the
//! OS config dir (`$RR_SETTINGS` overrides the path). A missing file means defaults; a broken
//! one means defaults and a warning, never a crash.

use crate::bindings::Bindings;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FOV_MIN: f32 = 60.0;
pub const FOV_MAX: f32 = 110.0;
pub const SENS_MIN: f32 = 0.0005;
pub const SENS_MAX: f32 = 0.01;

/// Everything the options menus edit; missing fields in a saved file fall back to defaults.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Radians of turn per mouse count.
    pub mouse_sensitivity: f32,
    pub invert_y: bool,
    /// Vertical field of view in degrees.
    pub fov_deg: f32,
    pub master: f32,
    pub sfx: f32,
    pub voice: f32,
    pub music: f32,
    pub low_res: bool,
    pub bindings: Bindings,
}

impl Default for Settings {
    fn default() -> Self {
        let v = crate::audio::AudioVolumes::default();
        Settings {
            mouse_sensitivity: 0.0025,
            invert_y: false,
            fov_deg: 75.0,
            master: v.master,
            sfx: v.sfx,
            voice: v.voice,
            music: v.music,
            low_res: false,
            bindings: Bindings::default(),
        }
    }
}

/// Where settings are read and written; `None` (demos, direct-level runs, tests) never touches disk.
#[derive(Resource, Debug, Clone, Default)]
pub struct SettingsPath(pub Option<PathBuf>);

impl Settings {
    /// `$RR_SETTINGS`, else `settings.ron` in the OS config dir.
    pub fn path() -> PathBuf {
        if let Ok(p) = std::env::var("RR_SETTINGS") {
            return p.into();
        }
        directories::ProjectDirs::from("", "", "rex-ruckus")
            .map(|d| d.config_dir().join("settings.ron"))
            .unwrap_or_else(|| PathBuf::from("settings.ron"))
    }

    /// Keeps every value in range (a hand-edited file may hold anything).
    pub fn clamp(mut self) -> Self {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        self.fov_deg = if self.fov_deg.is_finite() {
            self.fov_deg.clamp(FOV_MIN, FOV_MAX)
        } else {
            75.0
        };
        self.mouse_sensitivity = if self.mouse_sensitivity.is_finite() {
            self.mouse_sensitivity.clamp(SENS_MIN, SENS_MAX)
        } else {
            Settings::default().mouse_sensitivity
        };
        self.master = unit(self.master);
        self.sfx = unit(self.sfx);
        self.voice = unit(self.voice);
        self.music = unit(self.music);
        self.bindings.sanitise();
        self
    }

    /// Reads `path`: defaults if missing, defaults plus a warning text if unreadable or unparsable.
    pub fn load_from(path: &Path) -> (Settings, Option<String>) {
        let src = match std::fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (Settings::default(), None);
            }
            Err(e) => {
                return (
                    Settings::default(),
                    Some(format!("cannot read {}: {e}", path.display())),
                );
            }
        };
        match ron::from_str::<Settings>(&src) {
            Ok(s) => (s.clamp(), None),
            Err(e) => (
                Settings::default(),
                Some(format!("ignoring {}: {e}", path.display())),
            ),
        }
    }

    /// Writes `path` as pretty RON, creating the directory.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{Action, Binding};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tempdir() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rr-settings-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips_through_ron() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        let mut s = Settings {
            fov_deg: 100.0,
            ..Default::default()
        };
        s.bindings.bind(Action::Jump, Binding::Key(KeyCode::KeyX));
        s.save_to(&p).unwrap();
        let (back, warn) = Settings::load_from(&p);
        assert_eq!((back, warn), (s, None));
    }

    #[test]
    fn missing_file_is_silent_defaults() {
        let (s, warn) = Settings::load_from(Path::new("/nonexistent/rr/settings.ron"));
        assert_eq!((s, warn), (Settings::default(), None));
    }

    #[test]
    fn garbage_gives_defaults_and_a_warning() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        std::fs::write(&p, "(fov_deg: \"wide\", ").unwrap();
        let (s, warn) = Settings::load_from(&p);
        assert_eq!(s, Settings::default());
        assert!(warn.unwrap().contains("settings.ron"));
    }

    #[test]
    fn partial_file_keeps_its_fields_and_clamps() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        std::fs::write(&p, "(fov_deg: 500.0, invert_y: true)").unwrap();
        let (s, warn) = Settings::load_from(&p);
        assert!(s.invert_y);
        assert_eq!(s.fov_deg, FOV_MAX);
        assert_eq!(s.mouse_sensitivity, Settings::default().mouse_sensitivity);
        assert_eq!(warn, None);
    }

    #[test]
    fn unreadable_file_gives_defaults_and_a_warning() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        std::fs::write(&p, [0xff, 0xfe, 0x00]).unwrap();
        let (s, warn) = Settings::load_from(&p);
        assert_eq!(s, Settings::default());
        assert!(warn.unwrap().contains("settings.ron"));
    }
}
