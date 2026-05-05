use std::path::PathBuf;
#[allow(unused_imports, dead_code)]
use crate::common::*;
use std::process::Command;
fn glyim_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let bin = dir.join("glyim");
    if bin.exists() { Some(bin) } else { None }
}

#[test]
fn e2e_println_subprocess_stdout() {
    let bin = glyim_bin().expect("glyim binary not found");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.g");
    std::fs::write(&input, r#"main = () => { println("hello subprocess") }"#).unwrap();
    let output = Command::new(bin)
        .arg("run")
        .arg(&input)
        .output()
        .expect("glyim run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello subprocess"),
        "stdout should contain 'hello subprocess', got: {}",
        stdout
    );
    assert!(output.status.success(), "process should exit successfully");
}

