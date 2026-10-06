//! Level and sound tooling for Rex Ruckus: Meltdown.

pub mod audio;
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
    let issues = validate(&map);
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
}
