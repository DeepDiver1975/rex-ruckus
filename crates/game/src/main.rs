// Release builds are GUI apps on Windows: no console window behind the game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bevy::prelude::*;
use bevy::window::WindowResolution;
use rr_core::difficulty::Difficulty;
use rr_game::GamePlugin;
use rr_game::audio::AudioOptions;
use rr_game::demo::{DemoPlugin, load_script};
use rr_game::episode::load_episodes;
use rr_game::menu::text_field::clean_name;
use rr_game::net::NetStart;
use rr_net::{MAX_NAME, parse_addr};
use std::path::PathBuf;

const USAGE: &str = "usage: rex-ruckus [LEVEL] [--mute] [--difficulty easy|normal|hard]\n       [--demo SCRIPT [--record DIR]] [--partners 1|2|3]\n       [--host | --join ADDRESS] [--name NAME]";

/// Command line: `[LEVEL] [--mute] [--difficulty D] [--demo SCRIPT [--record DIR]]
/// [--partners N] [--host | --join ADDRESS] [--name NAME]`.
#[derive(Debug, PartialEq)]
struct Args {
    /// `None` plays the episode (or the depot, for a demo).
    level: Option<String>,
    demo: Option<PathBuf>,
    record: Option<PathBuf>,
    /// Start muted; implied by `--record`.
    mute: bool,
    difficulty: Difficulty,
    /// Dev aid: stand-in co-op partners to spawn (1-3), 0 for none.
    partners: u8,
    /// Host (the level, or the episode's first) or join a co-op game at once.
    net: Option<NetStart>,
    /// The co-op name for this run (not saved).
    name: Option<String>,
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
        level: None,
        demo: None,
        record: None,
        mute: false,
        difficulty: Difficulty::default(),
        partners: 0,
        net: None,
        name: None,
    };
    let mut host = false;
    let mut join = None;
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
            "--difficulty" => {
                let value = args.next().ok_or("--difficulty needs a value")?;
                out.difficulty = value.parse::<Difficulty>()?;
            }
            "--partners" => {
                out.partners = args
                    .next()
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|n| (1..=3).contains(n))
                    .ok_or("--partners needs 1, 2 or 3")?;
            }
            "--host" => host = true,
            "--join" => {
                let value = args.next().ok_or("--join needs an address")?;
                join = Some(parse_addr(&value).ok_or(format!("bad address {value:?}"))?);
            }
            "--name" => {
                let value = args
                    .next()
                    .and_then(|n| clean_name(&n))
                    .ok_or(format!("--name needs 1 to {MAX_NAME} bytes"))?;
                out.name = Some(value);
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => out.level = Some(arg),
        }
    }
    if out.record.is_some() && out.demo.is_none() {
        return Err("--record needs --demo".into());
    }
    out.net = match (host, join) {
        (true, Some(_)) => return Err("--host and --join exclude each other".into()),
        (true, None) => Some(NetStart::Host),
        (false, join) => join.map(NetStart::Join),
    };
    if out.net.is_some() && (out.demo.is_some() || out.partners > 0) {
        return Err("--host and --join exclude --demo and --partners".into());
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
        // X11 WM_CLASS and Wayland app_id; matches the Linux `.desktop` file.
        name: Some("rex-ruckus".into()),
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
    // Without a level or a demo the game plays the episode.
    let episode = (args.level.is_none() && demo.is_none()).then(load_episodes);
    // Direct-level dev runs and demos use defaults and never write the settings file.
    let settings = episode.is_some().then(rr_game::settings::Settings::path);
    let level = args.level.unwrap_or_else(|| "arsenal_depot.ron".into());
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
        .add_plugins(rr_game::icon::IconPlugin)
        .add_plugins(GamePlugin {
            level,
            episode,
            demo,
            audio,
            difficulty: args.difficulty,
            settings,
            partners: args.partners,
            net: args.net,
            name: args.name,
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
    fn defaults_to_the_episode() {
        let a = parse(&[]).unwrap();
        assert_eq!(a.level, None);
        assert_eq!(a.demo, None);
        assert_eq!(a.audio(), AudioOptions::default());
    }

    #[test]
    fn mute_starts_muted_with_music() {
        let a = parse(&["--mute", "test_yard.ron"]).unwrap();
        assert_eq!(a.level.as_deref(), Some("test_yard.ron"));
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
        assert_eq!(a.level.as_deref(), Some("test_yard.ron"));
        assert_eq!(a.demo, Some("d.ron".into()));
        assert_eq!(a.record, Some("out".into()));
    }

    #[test]
    fn difficulty_flag() {
        assert_eq!(parse(&[]).unwrap().difficulty, Difficulty::Normal);
        assert_eq!(
            parse(&["--difficulty", "hard"]).unwrap().difficulty,
            Difficulty::Hard
        );
        assert!(parse(&["--difficulty", "x"]).is_err());
        assert!(parse(&["--difficulty"]).is_err());
    }

    #[test]
    fn partners_flag() {
        assert_eq!(parse(&[]).unwrap().partners, 0);
        assert_eq!(parse(&["--partners", "2"]).unwrap().partners, 2);
        for bad in ["0", "4", "x"] {
            assert_eq!(
                parse(&["--partners", bad]),
                Err("--partners needs 1, 2 or 3".into())
            );
        }
        assert_eq!(
            parse(&["--partners"]),
            Err("--partners needs 1, 2 or 3".into())
        );
    }

    #[test]
    fn host_and_join_flags() {
        let a = parse(&["--host"]).unwrap();
        assert_eq!((a.net, a.level), (Some(NetStart::Host), None));
        let a = parse(&["--host", "test_yard.ron"]).unwrap();
        assert_eq!(a.level.as_deref(), Some("test_yard.ron"));
        let a = parse(&["--join", "10.0.0.2", "--name", "Ann"]).unwrap();
        assert_eq!(
            a.net,
            Some(NetStart::Join("10.0.0.2:27960".parse().unwrap()))
        );
        assert_eq!(a.name.as_deref(), Some("Ann"));
        assert_eq!(parse(&[]).unwrap().net, None);
    }

    #[test]
    fn rejects_bad_net_usage() {
        let long = "x".repeat(25);
        for bad in [
            &["--join"][..],
            &["--join", "x"],
            &["--host", "--join", "1.2.3.4"],
            &["--host", "--partners", "1"],
            &["--join", "1.2.3.4", "--demo", "d.ron"],
            &["--name", ""],
            &["--name", &long],
            &["--name"],
            &["--name", " \t "],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse(&["--name", &"x".repeat(24)]).is_ok());
        // The same filter as the name field: trimmed, control characters dropped.
        assert_eq!(
            parse(&["--name", " Ann\u{7} "]).unwrap().name.as_deref(),
            Some("Ann")
        );
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(parse(&["--demo"]).is_err());
        assert!(parse(&["--record", "out"]).is_err());
        assert!(parse(&["--fast"]).is_err());
    }
}
