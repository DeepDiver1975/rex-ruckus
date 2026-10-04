use std::path::{Path, PathBuf};

/// The `assets/` directory: `$RR_ASSETS`, else the workspace's `assets/` during development,
/// else `assets/` next to the executable (release bundles).
pub fn assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("RR_ASSETS") {
        return p.into();
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    if dev.is_dir() {
        return dev;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("assets")))
        .unwrap_or_else(|| PathBuf::from("assets"))
}
