//! Tests for source file collection (used by build and test commands).

use crate::commands::cmd_new;
use crate::config::NewOptions;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(base: &str) -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{base}-{id}")
}

#[test]
fn new_project_has_main_and_test_files() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("files-check");

    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path())).expect("new");

    assert!(result.path.join("src/main.g").exists());
    assert!(result.path.join("tests/integration.g").exists());
    assert!(result.path.join("Glyip.toml").exists());
    assert!(result.path.join("Glyip.lock").exists());
}

#[test]
fn new_project_has_no_lib_file_for_binary() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("bin-no-lib");

    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path())).expect("new");
    assert!(!result.path.join("src/lib.g").exists());
}

#[test]
fn new_lib_project_has_no_main_file() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("lib-no-main");

    let opts = NewOptions {
        lib: true,
        edition: "2024".to_string(),
    };
    let result = cmd_new(&name, &opts, Some(dir.path())).expect("new");
    assert!(!result.path.join("src/main.g").exists());
    assert!(result.path.join("src/lib.g").exists());
}

#[test]
fn nested_source_directories() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("nested");

    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path())).expect("new");

    // Create nested directories with source files
    let nested = result.path.join("src/submodule");
    std::fs::create_dir_all(&nested).expect("mkdir nested");
    std::fs::write(nested.join("helper.g"), "fn helper() {}").expect("write helper");

    assert!(nested.join("helper.g").exists());
}
