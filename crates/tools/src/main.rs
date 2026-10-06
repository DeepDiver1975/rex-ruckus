use clap::{Parser, Subcommand};
use rr_tools::{
    audio::validate_audio, models::validate_models, svg::render_svg, synth::synth_file,
    validate_episode, validate_file,
};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "rr-tools",
    about = "Level and sound tools for Rex Ruckus: Meltdown"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check levels and the audio content; exits non-zero if anything has an error.
    ///
    /// The audio and model checks (bank, quips, music, glTF scenes, provenance) run once, on the assets root taken as
    /// the parent of the first level's directory (`assets/levels/x.ron` -> `assets/`).
    Validate {
        #[arg(required = true)]
        levels: Vec<PathBuf>,
    },
    /// Draw a level top-down as SVG.
    RenderSvg {
        level: PathBuf,
        /// Output file (default: stdout).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Render a sound-effect recipe file to one WAV per recipe.
    Synth {
        recipes: PathBuf,
        /// Output directory.
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Print one `id<TAB>text` line per hero quip (input for scripts/gen-quips.sh).
    QuipsList {
        /// Quip table (default: assets/quips/quips.ron).
        #[arg(default_value = "assets/quips/quips.ron")]
        quips: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Validate { levels } => {
            let mut all_ok = true;
            for path in &levels {
                let (report, ok) = validate_file(path);
                print!("{report}");
                all_ok &= ok;
            }
            let assets = std::fs::canonicalize(&levels[0])
                .ok()
                .and_then(|p| p.ancestors().nth(2).map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("assets"));
            let music: Vec<String> = levels
                .iter()
                .filter_map(|p| std::fs::read_to_string(p).ok())
                .filter_map(|s| rr_core::map::Map::from_ron(&s).ok())
                .filter_map(|m| m.music)
                .collect();
            let (report, ok) = validate_audio(&assets, &music);
            print!("{report}");
            all_ok &= ok;
            let (report, ok) = validate_episode(&assets);
            print!("{report}");
            all_ok &= ok;
            let (report, ok) = validate_models(&assets);
            print!("{report}");
            all_ok &= ok;
            if all_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Cmd::RenderSvg { level, out } => {
            let map = match std::fs::read_to_string(&level)
                .map_err(|e| e.to_string())
                .and_then(|src| rr_core::map::Map::from_ron(&src).map_err(|e| e.to_string()))
            {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("{}: error: {e}", level.display());
                    return ExitCode::FAILURE;
                }
            };
            let svg = render_svg(&map);
            match out {
                Some(path) => {
                    if let Err(e) = std::fs::write(&path, svg) {
                        eprintln!("{}: error: {e}", path.display());
                        return ExitCode::FAILURE;
                    }
                }
                None => print!("{svg}"),
            }
            ExitCode::SUCCESS
        }
        Cmd::Synth { recipes, out } => match synth_file(&recipes, &out) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::QuipsList { quips } => {
            match std::fs::read_to_string(&quips)
                .map_err(|e| e.to_string())
                .and_then(|src| rr_core::audio::QuipTable::parse(&src).map_err(|e| e.to_string()))
            {
                Ok(table) => {
                    for q in table.quips {
                        println!("{}\t{}", q.id, q.text);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}: error: {e}", quips.display());
                    ExitCode::FAILURE
                }
            }
        }
    }
}
