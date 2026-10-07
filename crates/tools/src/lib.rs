//! Level and sound tooling for Rex Ruckus: Meltdown.

pub mod audio;
pub mod fonts;
pub mod info;
pub mod models;
pub mod svg;
pub mod synth;

use rr_core::map::Map;
use rr_core::validate::{has_errors, validate};
use std::fmt::Write;
use std::path::Path;

/// Parses and validates one level. Returns the printable report and whether it passed (no errors).
pub fn validate_source(label: &str, src: &str) -> (String, bool) {
    let map = match Map::from_ron(src) {
        Ok(m) => m,
        Err(e) => return (format!("{label}: error: {e}\n"), false),
    };
    let mut issues = validate(&map);
    for name in &map.materials {
        if !rr_core::map::KNOWN_MATERIALS.contains(&name.as_str()) {
            issues.push(rr_core::validate::Issue {
                severity: rr_core::validate::Severity::Error,
                message: format!("unknown material \"{name}\""),
            });
        }
    }
    let mut out = String::new();
    for i in &issues {
        writeln!(out, "{label}: {i}").unwrap();
    }
    let ok = !has_errors(&issues);
    writeln!(out, "{label}: {}", if ok { "ok" } else { "FAILED" }).unwrap();
    (out, ok)
}

pub fn validate_file(path: &Path) -> (String, bool) {
    let label = path.display().to_string();
    match std::fs::read_to_string(path) {
        Ok(src) => validate_source(&label, &src),
        Err(e) => (format!("{label}: error: cannot read: {e}\n"), false),
    }
}

/// The episode file's shape (mirrors `rr_game::episode::EpisodeDef`; the tools do not link the game).
#[derive(serde::Deserialize)]
struct EpisodeFile {
    levels: Vec<String>,
}

/// Checks `assets/episode.ron`: it parses, lists at least one level, and every listed level
/// exists under `assets/levels/` and validates.
pub fn validate_episode(assets: &Path) -> (String, bool) {
    let path = assets.join("episode.ron");
    let label = path.display().to_string();
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read: {e}"))
        .and_then(|src| ron::from_str::<EpisodeFile>(&src).map_err(|e| e.to_string()));
    let episode = match parsed {
        Ok(e) => e,
        Err(e) => return (format!("{label}: error: {e}\n"), false),
    };
    if episode.levels.is_empty() {
        return (format!("{label}: error: no levels listed\n"), false);
    }
    let mut out = String::new();
    let mut ok = true;
    for name in &episode.levels {
        let level = assets.join("levels").join(name);
        if !level.is_file() {
            writeln!(
                out,
                "{label}: error: level {name} not found in assets/levels/"
            )
            .unwrap();
            ok = false;
            continue;
        }
        let (report, level_ok) = validate_file(&level);
        out.push_str(&report);
        ok &= level_ok;
    }
    writeln!(out, "{label}: {}", if ok { "ok" } else { "FAILED" }).unwrap();
    (out, ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn level(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/levels")
            .join(name)
    }

    #[test]
    fn unknown_material_name_is_an_error() {
        let src = r#"(name: "t", materials: ["brick", "plaid"],
            vertices: [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)],
            sectors: [(loops: [[0, 1, 2, 3]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 1, wall_mat: 0)],
            player_start: (pos: (2.0, 2.0), angle_deg: 0.0))"#;
        let (report, ok) = validate_source("t.ron", src);
        assert!(!ok);
        assert!(report.contains(r#"unknown material "plaid""#), "{report}");
    }

    #[test]
    fn shipped_levels_use_known_materials_only() {
        for name in ["arsenal_depot.ron", "combat_arena.ron", "engine_lab.ron"] {
            let (report, ok) = validate_file(&level(name));
            assert!(ok, "{report}");
        }
    }

    #[test]
    fn shipped_levels_pass() {
        for name in ["test_yard.ron", "mechanics_lab.ron", "combat_arena.ron"] {
            let (report, ok) = validate_file(&level(name));
            assert!(ok, "{report}");
            assert!(report.ends_with(": ok\n"), "{report}");
        }
    }

    #[test]
    fn broken_level_fails_with_reason() {
        let (report, ok) = validate_source("bad.ron", "(nonsense");
        assert!(!ok);
        assert!(
            report.starts_with("bad.ron: error: RON parse error"),
            "{report}"
        );
        let (report, ok) = validate_file(Path::new("/nonexistent/level.ron"));
        assert!(!ok && report.contains("cannot read"), "{report}");
    }

    /// A temp assets tree with `levels/` holding a copy of the shipped test yard.
    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str, episode: &str, levels: &[&str]) -> Tree {
            let root =
                std::env::temp_dir().join(format!("rr-episode-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("levels")).unwrap();
            std::fs::write(root.join("episode.ron"), episode).unwrap();
            for l in levels {
                std::fs::copy(level("test_yard.ron"), root.join("levels").join(l)).unwrap();
            }
            Tree(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn good_episode_passes() {
        let t = Tree::new(
            "good",
            r#"(name: "x", levels: ["a.ron", "b.ron"])"#,
            &["a.ron", "b.ron"],
        );
        let (report, ok) = validate_episode(&t.0);
        assert!(ok, "{report}");
    }

    #[test]
    fn missing_level_fails() {
        let t = Tree::new(
            "missing",
            r#"(name: "x", levels: ["a.ron", "b.ron"])"#,
            &["a.ron"],
        );
        let (report, ok) = validate_episode(&t.0);
        assert!(!ok && report.contains("level b.ron not found"), "{report}");
    }

    #[test]
    fn empty_or_missing_episode_fails() {
        let t = Tree::new("empty", r#"(name: "x", levels: [])"#, &[]);
        let (report, ok) = validate_episode(&t.0);
        assert!(!ok && report.contains("no levels"), "{report}");
        let (report, ok) = validate_episode(Path::new("/nonexistent/assets"));
        assert!(!ok && report.contains("cannot read"), "{report}");
    }

    #[test]
    fn shipped_episode_passes() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let (report, ok) = validate_episode(&assets);
        assert!(ok, "{report}");
    }
}
