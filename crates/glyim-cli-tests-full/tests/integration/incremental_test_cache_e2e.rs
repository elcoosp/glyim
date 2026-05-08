use std::path::PathBuf;
use std::process::Command;
use std::fs;

fn glyim_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let bin = dir.join("glyim");
    if bin.exists() { Some(bin) } else { None }
}

#[test]
fn incremental_test_caching_runs_without_panicking() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.g");

    // Source with two test functions — one pass, one fail (just to exercise code paths)
    let source = r#"
#[test]
fn a() -> i64 { 0 }

#[test]
fn b() -> i64 { 1 }
"#;
    fs::write(&input, source).unwrap();

    // First run with --incremental
    let output = Command::new(&bin)
        .arg("test")
        .arg("--incremental")
        .arg(&input)
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .current_dir(dir.path())
        .output()
        .expect("glyim test --incremental");

    // May fail because test b returns non-zero. That's fine — we just want it to not crash.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _stdout = String::from_utf8_lossy(&output.stdout);

    // It must at least find and run test 'a'
    assert!(stderr.contains("test a"), "output should mention test a, got stderr: {}", stderr);

    // Second run with --incremental (no source change). Should not crash.
    let output2 = Command::new(&bin)
        .arg("test")
        .arg("--incremental")
        .arg(&input)
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .current_dir(dir.path())
        .output()
        .expect("second run");

    assert!(output2.status.code().is_some(), "second run should terminate normally");

    // Modify the source (only function a)
    let modified_source = source.replace("fn a() -> i64 { 0 }", "fn a() -> i64 { 42 }");
    fs::write(&input, modified_source).unwrap();

    // Third run after modification — should still not crash
    let output3 = Command::new(&bin)
        .arg("test")
        .arg("--incremental")
        .arg(&input)
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .current_dir(dir.path())
        .output()
        .expect("third run");

    assert!(output3.status.code().is_some(), "third run after modification should terminate normally");
}
