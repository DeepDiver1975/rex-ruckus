use clap::{Parser, Subcommand};
use rr_tools::{svg::render_svg, synth::synth_file, validate_file};
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
    /// Check levels; exits non-zero if any has an error.
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
    }
}
