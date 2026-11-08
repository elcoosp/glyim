//! Tests for the `glyip new` command (V20-T01).

use crate::commands::cmd_new;
use crate::config::{GlyipToml, NewOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(base: &str) -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{base}-{id}")
}

#[test]
fn new_creates_binary_project() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("my-app");
    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path())).expect("new");
    assert!(result.path.exists());
    assert!(result.path.join("Glyip.toml").exists());
    assert!(result.path.join("src/main.g").exists());
    assert!(result.path.join("tests/integration.g").exists());
    assert!(result.path.join("Glyip.lock").exists());

    let config = GlyipToml::read_from_dir(&result.path).expect("read config");
    assert_eq!(config.package.name, name);
    assert_eq!(config.package.version, "0.1.0");
    assert_eq!(config.package.edition, "2024");
    assert!(config.package.bin.is_some());
    assert!(config.package.lib.is_none());
}

#[test]
fn new_creates_library_project() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("my-lib");

    let opts = NewOptions {
        lib: true,
        edition: "2024".to_string(),
    };
    let result = cmd_new(&name, &opts, Some(dir.path())).expect("new");
    assert!(result.path.join("src/lib.g").exists());
    assert!(!result.path.join("src/main.g").exists());

    let config = GlyipToml::read_from_dir(&result.path).expect("read config");
    assert!(config.package.lib.is_some());
    assert!(config.package.bin.is_none());
}

#[test]
fn new_rejects_existing_directory() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("existing");

    // Pre-create the directory so cmd_new will reject it.
    let project_path = dir.path().join(&name);
    std::fs::create_dir_all(&project_path).expect("mkdir");

    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path()));
    assert!(result.is_err());
}

#[test]
fn new_entry_point_has_content() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("hello");

    let result = cmd_new(&name, &NewOptions::default(), Some(dir.path())).expect("new");
    let main_content = std::fs::read_to_string(result.path.join("src/main.g")).expect("read main");
    assert!(main_content.contains("fn main()"));
}

#[test]
fn new_with_custom_edition() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("ed2025");

    let opts = NewOptions {
        lib: false,
        edition: "2025".to_string(),
    };
    let result = cmd_new(&name, &opts, Some(dir.path())).expect("new");
    let config = GlyipToml::read_from_dir(&result.path).expect("read config");
    assert_eq!(config.package.edition, "2025");
}
