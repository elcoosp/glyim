use glyim_testr::compiler::Compiler;
use std::process::Command;

#[test]
fn compiled_binary_runs_test_via_env_and_outputs_pass() {
    let source = "#[test]\nfn my_test() -> i64 { 0 }";
    let artifact = Compiler::compile(source).expect("compile");
    let output = Command::new(&artifact.bin_path)
        .env("GLYIM_TEST", "my_test")
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS my_test"), "expected PASS, got: {}", stdout);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn compiled_binary_reports_fail_for_failing_test() {
    let source = "#[test]\nfn my_test() -> i64 { 1 }";
    let artifact = Compiler::compile(source).expect("compile");
    let output = Command::new(&artifact.bin_path)
        .env("GLYIM_TEST", "my_test")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FAIL my_test"), "expected FAIL, got: {}", stdout);
    assert_eq!(output.status.code(), Some(1));
}
