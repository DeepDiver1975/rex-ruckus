//! Font provenance: every shipped UI font must be named in CREDITS.md.

use std::fmt::Write;
use std::path::Path;

/// Checks every `fonts/*.ttf|otf` under `assets` is named in the CREDITS.md next to `assets/`.
/// No `fonts/` directory is fine.
pub fn validate_fonts(assets: &Path) -> (String, bool) {
    let dir = assets.join("fonts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return ("fonts: ok (none)\n".into(), true);
    };
    let credits = assets
        .parent()
        .map(|root| root.join("CREDITS.md"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            let n = n.to_lowercase();
            n.ends_with(".ttf") || n.ends_with(".otf")
        })
        .collect();
    names.sort();
    let mut out = String::new();
    let mut ok = true;
    for name in &names {
        let rel = format!("fonts/{name}");
        if !credits.contains(&format!("assets/{rel}")) {
            writeln!(
                out,
                "fonts: error: provenance: {rel} is not named in CREDITS.md"
            )
            .unwrap();
            ok = false;
        }
    }
    writeln!(out, "fonts: {}", if ok { "ok" } else { "FAILED" }).unwrap();
    (out, ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A temp tree `root/assets/fonts/x.ttf` with a CREDITS.md next to `assets/`.
    fn tree(tag: &str, credits: &str, font: bool) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rr-fonts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("CREDITS.md"), credits).unwrap();
        if font {
            std::fs::create_dir_all(root.join("assets/fonts")).unwrap();
            std::fs::write(root.join("assets/fonts/x.ttf"), b"x").unwrap();
        }
        root
    }

    #[test]
    fn unlisted_font_fails() {
        let root = tree("unlisted", "nothing", true);
        let (report, ok) = validate_fonts(&root.join("assets"));
        assert!(
            !ok && report.contains("provenance: fonts/x.ttf"),
            "{report}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn listed_font_passes() {
        let root = tree("listed", "| `assets/fonts/x.ttf` | f |", true);
        let (report, ok) = validate_fonts(&root.join("assets"));
        assert!(ok, "{report}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_fonts_dir_passes() {
        let root = tree("none", "", false);
        let (report, ok) = validate_fonts(&root.join("assets"));
        assert!(ok, "{report}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shipped_fonts_pass() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let (report, ok) = validate_fonts(&assets);
        assert!(ok, "{report}");
    }
}
