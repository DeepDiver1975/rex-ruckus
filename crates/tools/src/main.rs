use clap::{Parser, Subcommand};
use rr_core::map::{Map, RawLevel};
use rr_tools::build::{build_source_with_ids, name_sectors};
use rr_tools::{
    audio::validate_audio,
    fonts::validate_fonts,
    models::validate_models,
    svg::{SvgOptions, render_svg_with},
    synth::synth_file,
    validate_episode, validate_file, validate_source,
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
    /// Print a level's sectors, channels and counts.
    Info { level: PathBuf },
    /// Draw a level top-down as SVG.
    RenderSvg {
        level: PathBuf,
        /// Label every vertex with its index.
        #[arg(long)]
        ids: bool,
        /// Output file (default: stdout).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Compile level sources (levels/src/*.ron) into game levels; output that fails to
    /// validate is not written. --check verifies the committed output is up to date instead
    /// of writing it, and flags built levels in the output directory with no given source.
    Build {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long)]
        check: bool,
        #[arg(long, default_value = "assets/levels")]
        out_dir: PathBuf,
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

/// Reads a level file as its raw form and the built map.
fn load_raw(path: &std::path::Path) -> Result<(RawLevel, Map), String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw: RawLevel = ron::from_str(&src).map_err(|e| e.to_string())?;
    let map = Map::from_raw(raw.clone()).map_err(|e| e.to_string())?;
    Ok((raw, map))
}

/// The line every built level carries; hand-written levels lack it.
const BUILT_MARKER: &str = "Built by `rr-tools build`";

/// Builds one source; returns whether it is fine (and, without `check`, written). Validation
/// messages are labelled with the source path and name sectors by their source ids. Output that
/// fails to load or validate is not written, so a broken build never replaces a good level.
fn build_one(path: &std::path::Path, out_dir: &std::path::Path, check: bool) -> bool {
    let (Some(name), Some(stem)) = (path.file_name(), path.file_stem()) else {
        eprintln!("{}: error: not a file path", path.display());
        return false;
    };
    let built = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|src| build_source_with_ids(&src, &stem.to_string_lossy()));
    let built = match built {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: error: {e}", path.display());
            return false;
        }
    };
    let out_path = out_dir.join(name);
    let (report, mut ok) = validate_source(&path.display().to_string(), &built.text);
    print!("{}", name_sectors(&report, &built.sector_ids));
    if check {
        if std::fs::read_to_string(&out_path).ok().as_deref() != Some(built.text.as_str()) {
            eprintln!("{}: out of date; run rr-tools build", out_path.display());
            ok = false;
        }
    } else if !ok {
        eprintln!("{}: not written: the build failed", out_path.display());
    } else if let Err(e) = std::fs::write(&out_path, &built.text) {
        eprintln!("{}: error: {e}", out_path.display());
        ok = false;
    }
    ok
}

/// Built levels in `out_dir` (they carry [`BUILT_MARKER`]) whose source is not among `sources`:
/// a renamed or deleted source leaves its output behind. Returns whether there are none.
fn check_orphans(sources: &[PathBuf], out_dir: &std::path::Path) -> bool {
    let names: std::collections::HashSet<_> =
        sources.iter().filter_map(|p| p.file_name()).collect();
    let Ok(entries) = std::fs::read_dir(out_dir) else {
        eprintln!(
            "{}: error: cannot read the output directory",
            out_dir.display()
        );
        return false;
    };
    let mut orphans: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .filter(|p| p.file_name().is_some_and(|n| !names.contains(n)))
        .filter(|p| std::fs::read_to_string(p).is_ok_and(|s| s.contains(BUILT_MARKER)))
        .collect();
    orphans.sort();
    for p in &orphans {
        eprintln!("{}: built level has no source in levels/src", p.display());
    }
    orphans.is_empty()
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Build {
            sources,
            check,
            out_dir,
        } => {
            let mut all_ok = true;
            for path in &sources {
                all_ok &= build_one(path, &out_dir, check);
            }
            if check {
                all_ok &= check_orphans(&sources, &out_dir);
            }
            if all_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
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
            let (report, ok) = validate_fonts(&assets);
            print!("{report}");
            all_ok &= ok;
            if all_ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Cmd::Info { level } => match load_raw(&level) {
            Ok((raw, map)) => {
                print!("{}", rr_tools::info::level_info(&raw, &map));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}: error: {e}", level.display());
                ExitCode::FAILURE
            }
        },
        Cmd::RenderSvg { level, ids, out } => {
            let (raw, map) = match load_raw(&level) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}: error: {e}", level.display());
                    return ExitCode::FAILURE;
                }
            };
            let opts = SvgOptions {
                vertices: ids.then_some(raw.vertices.as_slice()),
            };
            let svg = render_svg_with(&map, &opts);
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
