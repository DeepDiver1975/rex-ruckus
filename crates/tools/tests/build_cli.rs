use std::process::Command;

fn rr_tools() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rr-tools"))
}

const SRC: &str = r#"// cli test
(name: "Cli", defaults: (floor_z: 0.0, ceil_z: 3.0, floor: "concrete", ceil: "metal", wall: "brick"),
 sectors: [(id: "a", rect: (0.0, 0.0, 4.0, 4.0)), (id: "b", rect: (4.0, 0.0, 8.0, 4.0))],
 switches: [(wall: ((8.0, 0.0), (8.0, 4.0)), sector: "b", action: Exit)],
 player_start: (pos: (1.0, 2.0), angle_deg: 0.0))"#;

#[test]
fn build_writes_then_check_passes_and_detects_drift() {
    let dir = std::env::temp_dir().join(format!("rr-build-{}-{}", std::process::id(), line!()));
    let (src_dir, out_dir) = (dir.join("src"), dir.join("out"));
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();
    let src = src_dir.join("cli.ron");
    std::fs::write(&src, SRC).unwrap();

    let run = |check: bool| {
        let mut c = rr_tools();
        c.arg("build").arg("--out-dir").arg(&out_dir);
        if check {
            c.arg("--check");
        }
        c.arg(&src).status().unwrap().success()
    };
    assert!(!run(true), "check fails before the first build");
    assert!(run(false), "build succeeds");
    let out = std::fs::read_to_string(out_dir.join("cli.ron")).unwrap();
    assert!(out.contains("// 1 b"));
    assert!(run(true), "check passes on fresh output");
    std::fs::write(
        out_dir.join("cli.ron"),
        out.replace("ceil_z: 3.0", "ceil_z: 3.5"),
    )
    .unwrap();
    assert!(!run(true), "check catches a hand edit");
    std::fs::remove_dir_all(&dir).ok();
}

/// A fresh `src/` and `out/` pair under the temp dir.
fn dirs(tag: u32) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("rr-build-{}-{tag}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let (src_dir, out_dir) = (dir.join("src"), dir.join("out"));
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();
    (dir, src_dir, out_dir)
}

#[test]
fn failing_build_keeps_the_old_output_and_names_source_ids() {
    let (dir, src_dir, out_dir) = dirs(line!());
    let src = src_dir.join("cli.ron");
    let good = out_dir.join("cli.ron");
    std::fs::write(&good, "// the committed level\n").unwrap();
    // "annex" overlaps "lobby": the build validates, fails, and must not write.
    std::fs::write(
        &src,
        SRC.replace(
            r#"(id: "a", rect: (0.0, 0.0, 4.0, 4.0)), (id: "b", rect: (4.0, 0.0, 8.0, 4.0))"#,
            r#"(id: "lobby", rect: (0.0, 0.0, 4.0, 4.0)), (id: "annex", rect: (2.0, 1.0, 8.0, 3.0))"#,
        )
        .replace(
            r#"wall: ((8.0, 0.0), (8.0, 4.0)), sector: "b""#,
            r#"wall: ((8.0, 1.0), (8.0, 3.0)), sector: "annex""#,
        ),
    )
    .unwrap();
    let out = rr_tools()
        .arg("build")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg(&src)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stdout}");
    assert!(
        stdout.contains(&format!("{}: ", src.display()))
            && stdout.contains(r#"0 "lobby""#)
            && stdout.contains(r#"1 "annex""#),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(&good).unwrap(),
        "// the committed level\n"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn check_flags_a_built_level_without_a_source() {
    let (dir, src_dir, out_dir) = dirs(line!());
    let src = src_dir.join("cli.ron");
    std::fs::write(&src, SRC).unwrap();
    let build = |check: bool| {
        let mut c = rr_tools();
        c.arg("build").arg("--out-dir").arg(&out_dir);
        if check {
            c.arg("--check");
        }
        c.arg(&src).output().unwrap()
    };
    assert!(build(false).status.success());
    // A hand-written level has no "Built by" line and is left alone.
    std::fs::write(out_dir.join("dev.ron"), "// hand-written\n()\n").unwrap();
    assert!(
        build(true).status.success(),
        "hand-written levels are not orphans"
    );
    // The output of a renamed source stays behind.
    std::fs::copy(out_dir.join("cli.ron"), out_dir.join("old_name.ron")).unwrap();
    let out = build(true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("old_name.ron: built level has no source in levels/src"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
