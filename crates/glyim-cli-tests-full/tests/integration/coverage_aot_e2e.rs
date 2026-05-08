use glyim_compiler::pipeline::{self, BuildMode};
use std::path::PathBuf;
use std::process::Command;

fn temp_g(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.g");
    std::fs::write(&path, content).unwrap();
    Box::leak(Box::new(dir));
    path
}

#[test]
fn coverage_aot_run_produces_dump() {
    let dir = tempfile::tempdir().unwrap();
    let source = "fn main() -> i64 { let mut i = 0; while i < 2 { i = i + 1 }; 0 }";
    let input = temp_g(source);
    let output = dir.path().join("test_bin");
    let cov_path = dir.path().join("glyim-cov.json");

    let bin = pipeline::build_with_mode(
        &input,
        Some(&output),
        BuildMode::Debug,
        None,
        None,
        true,
        false,
    )
    .expect("build should succeed");

    let mut cmd = Command::new(&bin);
    cmd.current_dir(&dir);
    let status = cmd.status().expect("binary execution failed");
    assert!(status.success(), "binary should exit successfully");
    assert!(cov_path.exists(), "coverage file should exist");
    let data = std::fs::read_to_string(&cov_path).unwrap();
    let dump: glyim_coverage::data::CoverageDump = serde_json::from_str(&data).unwrap();
    assert!(!dump.counters.is_empty(), "counters should not be empty");
}
