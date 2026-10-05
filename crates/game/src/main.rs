// Release builds are GUI apps on Windows: no console window behind the game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
use bevy::window::WindowResolution;
use rr_game::GamePlugin;
use rr_game::demo::{DemoPlugin, load_script};
use std::path::PathBuf;

/// Command line: `[LEVEL] [--demo SCRIPT [--record DIR]]`.
#[derive(Debug, PartialEq)]
struct Args {
    level: String,
    demo: Option<PathBuf>,
    record: Option<PathBuf>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        level: "arsenal_depot.ron".into(),
        demo: None,
        record: None,
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
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => out.level = arg,
        }
    }
    if out.record.is_some() && out.demo.is_none() {
        return Err("--record needs --demo".into());
    }
    Ok(out)
}

fn main() {
    let args = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("rex-ruckus: {e}\nusage: rex-ruckus [LEVEL] [--demo SCRIPT [--record DIR]]");
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
    let demo = args.demo.map(|script| DemoPlugin {
        script: load_script(&script),
        record: args.record,
    });
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                }),
        )
        .add_plugins(GamePlugin {
            level: args.level,
            demo,
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
