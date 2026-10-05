// Release builds are GUI apps on Windows: no console window behind the game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
use bevy::window::WindowResolution;
use rr_game::GamePlugin;
use rr_game::audio::AudioOptions;
use rr_game::demo::{DemoPlugin, load_script};
use std::path::PathBuf;

const USAGE: &str = "usage: rex-ruckus [LEVEL] [--mute] [--demo SCRIPT [--record DIR]]";

/// Command line: `[LEVEL] [--mute] [--demo SCRIPT [--record DIR]]`.
#[derive(Debug, PartialEq)]
struct Args {
    level: String,
    demo: Option<PathBuf>,
    record: Option<PathBuf>,
    /// Start muted; implied by `--record`.
    mute: bool,
}

impl Args {
    /// A recording runs on manual time, so sound would drift: it is muted and has no music.
    fn audio(&self) -> AudioOptions {
        AudioOptions {
            muted: self.mute,
            music: self.record.is_none(),
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        level: "arsenal_depot.ron".into(),
        demo: None,
        record: None,
        mute: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" | "--record" => {
                let value = args.next().ok_or(format!("{arg} needs a path"))?.into();
                if arg == "--demo" {
                    out.demo = Some(value);
                } else {
                    out.record = Some(value);
                }
            }
            "--mute" => out.mute = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => out.level = arg,
        }
    }
    if out.record.is_some() && out.demo.is_none() {
        return Err("--record needs --demo".into());
    }
    out.mute |= out.record.is_some();
    Ok(out)
}

fn main() {
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("rex-ruckus: {e}\n{USAGE}");
        std::process::exit(2);
    });
    let mut window = Window {
        title: "Rex Ruckus: Meltdown".into(),
        ..default()
    };
    // Demo runs get a fixed size so recordings are reproducible.
    if args.demo.is_some() {
        window.resolution = WindowResolution::new(1280, 720);
        window.resizable = false;
    }
    let audio = args.audio();
    let demo = args.demo.map(|script| DemoPlugin {
        script: load_script(&script),
        record: args.record,
    });
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(AssetPlugin {
                    file_path: rr_game::paths::assets_dir().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin {
            level: args.level,
            demo,
            audio,
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_to_the_depot_without_a_demo() {
        let a = parse(&[]).unwrap();
        assert_eq!(a.level, "arsenal_depot.ron");
        assert_eq!(a.demo, None);
        assert_eq!(a.audio(), AudioOptions::default());
    }

    #[test]
    fn mute_starts_muted_with_music() {
        let a = parse(&["--mute", "test_yard.ron"]).unwrap();
        assert_eq!(a.level, "test_yard.ron");
        assert_eq!(
            a.audio(),
            AudioOptions {
                muted: true,
                music: true
            }
        );
    }

    #[test]
    fn record_is_muted_without_music() {
        let a = parse(&["--demo", "d.ron", "--record", "out"]).unwrap();
        assert!(a.mute);
        assert_eq!(
            a.audio(),
            AudioOptions {
                muted: true,
                music: false
            }
        );
        // A plain demo keeps its sound.
        assert_eq!(
            parse(&["--demo", "d.ron"]).unwrap().audio(),
            AudioOptions::default()
        );
    }

    #[test]
    fn reads_level_demo_and_record_in_any_order() {
        let a = parse(&["--record", "out", "test_yard.ron", "--demo", "d.ron"]).unwrap();
        assert_eq!(a.level, "test_yard.ron");
        assert_eq!(a.demo, Some("d.ron".into()));
        assert_eq!(a.record, Some("out".into()));
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(parse(&["--demo"]).is_err());
        assert!(parse(&["--record", "out"]).is_err());
        assert!(parse(&["--fast"]).is_err());
    }
}
