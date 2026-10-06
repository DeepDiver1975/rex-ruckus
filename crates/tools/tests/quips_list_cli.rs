//! End-to-end checks of the `rr-tools quips-list` subcommand.

use std::process::Command;

#[test]
fn prints_one_id_tab_text_line_per_quip() {
    let table = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/quips/quips.ron");
    let src = std::fs::read_to_string(table).unwrap();
    let expected = rr_core::audio::QuipTable::parse(&src).unwrap().quips;
    let out = Command::new(env!("CARGO_BIN_EXE_rr-tools"))
        .args(["quips-list", table])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (line, q) in lines.iter().zip(&expected) {
        assert_eq!(*line, format!("{}\t{}", q.id, q.text));
    }
}

#[test]
fn missing_file_exits_non_zero() {
    let out = Command::new(env!("CARGO_BIN_EXE_rr-tools"))
        .args(["quips-list", "/nonexistent/quips.ron"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
