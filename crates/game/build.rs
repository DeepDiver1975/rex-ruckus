//! Embeds the icon and version info in the Windows exe (Explorer, shortcuts, Task Manager).
//! Other targets get the icon at runtime only (`src/icon.rs`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../installer/rex-ruckus.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        windows_resources();
    }
}

#[cfg(windows)]
fn windows_resources() {
    let version = env!("CARGO_PKG_VERSION");
    winresource::WindowsResource::new()
        .set_icon("../../installer/rex-ruckus.ico")
        .set("ProductName", "Rex Ruckus: Meltdown")
        .set("FileDescription", "Rex Ruckus: Meltdown")
        .set("CompanyName", "Rex Ruckus contributors")
        .set(
            "LegalCopyright",
            "Rex Ruckus contributors, GPL-3.0-or-later",
        )
        .set("OriginalFilename", "rex-ruckus.exe")
        .set("ProductVersion", version)
        .set("FileVersion", version)
        .compile()
        .expect("embedding the Windows icon and version info");
}
