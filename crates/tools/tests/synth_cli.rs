//! End-to-end checks of the `rr-tools synth` subcommand.

use std::process::Command;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rr-synth-cli-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn malformed_recipe_exits_non_zero_without_panicking() {
    let dir = temp_dir("bad");
    let recipes = dir.join("bad.ron");
    std::fs::write(&recipes, "[(name: \"x\", layers: [(wave: Kazoo)])]").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rr-tools"))
        .args([
            "synth".as_ref(),
            recipes.as_os_str(),
            "-o".as_ref(),
            dir.join("out").as_os_str(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn writes_wavs_and_prints_one_line_each() {
    let dir = temp_dir("ok");
    let recipes = dir.join("ok.ron");
    std::fs::write(
        &recipes,
        r#"[
            (name: "a", layers: [(wave: Sine, freq: 440.0, sustain: 0.1, gain: 1.0)]),
            (name: "b", layers: [(wave: Noise, freq: 1000.0, decay: 0.1, gain: 1.0)], seed: 3),
        ]"#,
    )
    .unwrap();
    let out_dir = dir.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_rr-tools"))
        .args([
            "synth".as_ref(),
            recipes.as_os_str(),
            "-o".as_ref(),
            out_dir.as_os_str(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    assert!(out_dir.join("a.wav").is_file() && out_dir.join("b.wav").is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}
