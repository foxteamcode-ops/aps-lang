//! Regression tests for deterministic drops of non-escaping `Vec` locals.
//! Builds repro programs with heap accounting enabled (`APS_ALLOC_STATS=1`)
//! and checks both program output and leak counters.

use std::path::PathBuf;
use std::process::Command;

fn build_and_run(src: &str, exe_name: &str) -> (String, String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = std::env::temp_dir().join("aps_drop_tests");
    std::fs::create_dir_all(&out_dir).unwrap();
    let exe = out_dir.join(exe_name);
    let build = Command::new(env!("CARGO_BIN_EXE_aps"))
        .current_dir(&root)
        .env("APS_ALLOC_STATS", "1")
        .args(["build", src, "-o"])
        .arg(&exe)
        .args(["-O", "3"])
        .output()
        .expect("failed to run aps build");
    assert!(
        build.status.success(),
        "build failed: {}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("failed to run program");
    assert!(run.status.success(), "program exited with failure");
    (
        String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&run.stderr).to_string(),
    )
}

fn leaked(stderr: &str) -> i64 {
    let line = stderr
        .lines()
        .find(|l| l.contains("[aps-alloc-stats]"))
        .expect("missing alloc stats line");
    line.split("leaked=").nth(1).unwrap().trim().parse().unwrap()
}

#[test]
fn test_vec_local_in_loop_is_freed() {
    let (out, err) = build_and_run("tests/repro/leak_vec_loop.aps", "leak_vec_loop.exe");
    assert_eq!(out.trim(), "10000");
    assert_eq!(leaked(&err), 0, "non-escaping Vec locals must not leak: {}", err);
}

#[test]
fn test_escaping_and_control_flow_cases_stay_correct() {
    let (out, err) = build_and_run("tests/repro/drop_cases.aps", "drop_cases.exe");
    assert_eq!(out, "5\n8\n4\n2\n6\n2\n2000\n");
    // Only the Vec returned from `make_escaping` (header + buffer) may remain allocated.
    assert!(leaked(&err) <= 2, "unexpected leaks: {}", err);
}
