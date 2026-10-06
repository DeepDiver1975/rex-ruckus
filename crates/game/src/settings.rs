//! Player settings: mouse, field of view, volumes, low-res and key bindings, saved as RON in the
//! OS config dir (`$RR_SETTINGS` overrides the path). A missing file means defaults; a broken
//! one means defaults and a warning, never a crash.

use crate::audio::AudioVolumes;
use crate::bindings::Bindings;
use crate::mechanics::HudMessage;
use crate::player::{LookSettings, PlayerCamera};
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

/// Set to `true` (by the options menu) to write [`Settings`] to [`SettingsPath`]; cleared after.
#[derive(Resource, Debug, Default)]
pub struct SaveSettings(pub bool);

/// Loads [`Settings`] from `path` (`None`: defaults, never touches disk), applies them to the
/// look, audio, binding and camera state, and saves on request. Add it before the plugins whose
/// "insert if absent" defaults would otherwise win.
pub struct SettingsPlugin {
    pub path: Option<PathBuf>,
}

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let (settings, warning) = match &self.path {
            Some(p) => Settings::load_from(p),
            None => (Settings::default(), None),
        };
        if let Some(w) = warning {
            warn!("{w}");
        }
        // The saved mix is the initial one, so startup spawns (level music) already use it.
        app.insert_resource(AudioVolumes {
            master: settings.master,
            sfx: settings.sfx,
            voice: settings.voice,
            music: settings.music,
            muted: false,
        })
        .insert_resource(LookSettings {
            sensitivity: settings.mouse_sensitivity,
            invert_y: settings.invert_y,
        })
        .insert_resource(settings.bindings.clone())
        .insert_resource(settings)
        .insert_resource(SettingsPath(self.path.clone()))
        .init_resource::<SaveSettings>()
        .add_systems(
            Update,
            (
                apply_settings.run_if(resource_changed::<Settings>),
                apply_fov,
                save_settings.run_if(|f: Res<SaveSettings>| f.0),
            ),
        );
    }
}

/// Copies [`Settings`] into the live look, binding and volume resources (the mute toggle stays).
pub fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    volumes: Option<ResMut<AudioVolumes>>,
) {
    commands.insert_resource(LookSettings {
        sensitivity: settings.mouse_sensitivity,
        invert_y: settings.invert_y,
    });
    commands.insert_resource(settings.bindings.clone());
    if let Some(mut volumes) = volumes {
        let next = AudioVolumes {
            master: settings.master,
            sfx: settings.sfx,
            voice: settings.voice,
            music: settings.music,
            muted: volumes.muted,
        };
        volumes.set_if_neq(next);
    }
}

/// Keeps the player camera's field of view on the setting, also for a camera spawned later.
fn apply_fov(settings: Res<Settings>, mut cameras: Query<&mut Projection, With<PlayerCamera>>) {
    let fov = settings.fov_deg.to_radians();
    for mut proj in &mut cameras {
        if let Projection::Perspective(p) = &*proj
            && p.fov != fov
            && let Projection::Perspective(p) = &mut *proj
        {
            p.fov = fov;
        }
    }
}

/// Writes the settings file once per request; a failure is logged and shown on the HUD.
fn save_settings(
    settings: Res<Settings>,
    path: Res<SettingsPath>,
    mut flag: ResMut<SaveSettings>,
    msg: Option<ResMut<HudMessage>>,
) {
    flag.0 = false;
    let Some(path) = &path.0 else { return };
    if let Err(e) = settings.save_to(path) {
        warn!("cannot save {}: {e}", path.display());
        if let Some(mut msg) = msg {
            msg.show("Could not save settings");
        }
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

    #[test]
    fn settings_drive_look_volumes_and_bindings() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut s = Settings {
            mouse_sensitivity: 0.004,
            invert_y: true,
            music: 0.2,
            ..Default::default()
        };
        s.bindings.bind(Action::Jump, Binding::Key(KeyCode::KeyX));
        app.insert_resource(s.clone())
            .add_systems(Update, apply_settings);
        app.insert_resource(AudioVolumes {
            muted: true,
            ..Default::default()
        });
        app.update();
        let look = app.world().resource::<LookSettings>();
        assert_eq!((look.sensitivity, look.invert_y), (0.004, true));
        let v = app.world().resource::<AudioVolumes>();
        assert_eq!((v.music, v.muted), (0.2, true));
        assert_eq!(app.world().resource::<Bindings>(), &s.bindings);
    }

    #[test]
    fn fov_follows_the_setting_even_for_a_late_camera() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SettingsPlugin { path: None });
        app.world_mut().resource_mut::<Settings>().fov_deg = 100.0;
        app.update();
        let cam = app
            .world_mut()
            .spawn((PlayerCamera, Projection::Perspective(default())))
            .id();
        app.update();
        let Projection::Perspective(p) = app.world().get::<Projection>(cam).unwrap() else {
            panic!("perspective")
        };
        assert!((p.fov - 100f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn save_flag_writes_the_file_once() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(SettingsPlugin {
            path: Some(p.clone()),
        });
        app.world_mut().resource_mut::<Settings>().fov_deg = 90.0;
        app.world_mut().resource_mut::<SaveSettings>().0 = true;
        app.update();
        assert_eq!(Settings::load_from(&p).0.fov_deg, 90.0);
        assert!(!app.world().resource::<SaveSettings>().0);
    }

    #[test]
    fn no_path_never_writes() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SettingsPlugin { path: None });
        app.world_mut().resource_mut::<SaveSettings>().0 = true;
        app.update();
        assert!(!app.world().resource::<SaveSettings>().0);
    }

    #[test]
    fn saved_volumes_are_the_initial_mix_and_mute_stays_with_the_audio_plugin() {
        let dir = tempdir();
        let p = dir.join("settings.ron");
        Settings {
            music: 0.15,
            ..Default::default()
        }
        .save_to(&p)
        .unwrap();
        let mut app = App::new();
        app.add_plugins(SettingsPlugin { path: Some(p) });
        assert_eq!(app.world().resource::<AudioVolumes>().music, 0.15);
        app.add_plugins(crate::audio::AudioFxPlugin {
            options: crate::audio::AudioOptions {
                muted: true,
                music: true,
            },
            scripted: true,
        });
        let v = app.world().resource::<AudioVolumes>();
        assert_eq!((v.music, v.muted), (0.15, true));
    }

    #[test]
    fn failed_save_clears_the_flag_without_panicking() {
        let dir = tempdir();
        let blocker = dir.join("file");
        std::fs::write(&blocker, "x").unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(SettingsPlugin {
            path: Some(blocker.join("settings.ron")),
        });
        app.init_resource::<HudMessage>();
        app.world_mut().resource_mut::<SaveSettings>().0 = true;
        app.update();
        assert!(!app.world().resource::<SaveSettings>().0);
        assert_eq!(
            app.world().resource::<HudMessage>().text,
            "Could not save settings"
        );
    }
}
