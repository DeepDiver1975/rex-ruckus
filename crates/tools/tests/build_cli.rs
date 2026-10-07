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
