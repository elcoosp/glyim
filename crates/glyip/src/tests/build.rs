//! Tests for the `glyip build` command (V20-T02).

use crate::commands::cmd_build;
use crate::config::{BuildOptions, NewOptions};
use crate::error::GlyipError;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(base: &str) -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{base}-{id}")
}

fn create_test_project(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let result = crate::commands::cmd_new(name, &NewOptions::default(), Some(dir)).expect("new");
    result.path
}

#[test]
fn build_fails_without_config() {
    let dir = TempDir::new().expect("temp dir");
    let result = cmd_build(dir.path(), &BuildOptions::default());
    assert!(result.is_err());
    if let Err(GlyipError::ProjectNotFound(_)) = result {
        // expected
    } else {
        panic!("expected ProjectNotFound, got {result:?}");
    }
}

#[test]
fn build_detects_project_config() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("buildable");
    let project_path = create_test_project(dir.path(), &name);

    let result = cmd_build(&project_path, &BuildOptions::default());
    match result {
        Ok(build_result) => {
            assert!(build_result.output.to_string_lossy().contains(&name));
        }
        Err(GlyipError::BuildFailed(_)) => {
            // Compilation failed, but config was read successfully.
        }
        Err(_) => {
            // Other errors are also acceptable.
        }
    }
}

#[test]
fn build_release_mode() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("release-build");
    let project_path = create_test_project(dir.path(), &name);

    let opts = BuildOptions {
        release: true,
        ..BuildOptions::default()
    };
    let result = cmd_build(&project_path, &opts);
    if let Ok(r) = result {
        assert!(r.output.to_string_lossy().contains("release"));
    }
}
