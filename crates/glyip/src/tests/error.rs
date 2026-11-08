//! Tests for error types and Display implementations.

use crate::error::{GlyipError, GlyipResult};
use std::error::Error;
use std::path::PathBuf;

#[test]
fn io_error_display() {
    let err = GlyipError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file missing",
    ));
    let msg = format!("{err}");
    assert!(msg.contains("I/O error"));
    assert!(msg.contains("file missing"));
}

#[test]
fn config_parse_error_display() {
    let err = GlyipError::ConfigParse("bad syntax at line 5".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("config parse error"));
    assert!(msg.contains("bad syntax at line 5"));
}

#[test]
fn config_validation_error_display() {
    let err = GlyipError::ConfigValidation("name is required".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("config validation error"));
    assert!(msg.contains("name is required"));
}

#[test]
fn dependency_cycle_error_display() {
    let err = GlyipError::DependencyCycle(vec!["a".to_string(), "b".to_string(), "a".to_string()]);
    let msg = format!("{err}");
    assert!(msg.contains("dependency cycle"));
    assert!(msg.contains("a -> b -> a"));
}

#[test]
fn dependency_not_found_with_version_display() {
    let err = GlyipError::DependencyNotFound {
        name: "serde".to_string(),
        version: Some("1.0".to_string()),
    };
    let msg = format!("{err}");
    assert!(msg.contains("serde"));
    assert!(msg.contains("1.0"));
}

#[test]
fn dependency_not_found_without_version_display() {
    let err = GlyipError::DependencyNotFound {
        name: "missing".to_string(),
        version: None,
    };
    let msg = format!("{err}");
    assert!(msg.contains("missing"));
    assert!(!msg.contains('v'));
}

#[test]
fn build_failed_display() {
    let err = GlyipError::BuildFailed(vec![]);
    let msg = format!("{err}");
    assert!(msg.contains("build failed"));
}

#[test]
fn cache_corrupted_display() {
    let err = GlyipError::CacheCorrupted("hash mismatch".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("cache corrupted"));
    assert!(msg.contains("hash mismatch"));
}

#[test]
fn project_not_found_display() {
    let err = GlyipError::ProjectNotFound(PathBuf::from("/tmp/myproject"));
    let msg = format!("{err}");
    assert!(msg.contains("project not found"));
    assert!(msg.contains("myproject"));
}

#[test]
fn project_already_exists_display() {
    let err = GlyipError::ProjectAlreadyExists(PathBuf::from("/tmp/existing"));
    let msg = format!("{err}");
    assert!(msg.contains("already exists"));
    assert!(msg.contains("existing"));
}

#[test]
fn lockfile_conflict_display() {
    let err = GlyipError::LockfileConflict("version mismatch".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("lockfile conflict"));
}

#[test]
fn no_entry_point_display() {
    let err = GlyipError::NoEntryPoint(PathBuf::from("/tmp/proj"));
    let msg = format!("{err}");
    assert!(msg.contains("no entry point"));
    assert!(msg.contains("proj"));
}

#[test]
fn other_error_display() {
    let err = GlyipError::Other("something went wrong".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("something went wrong"));
}

#[test]
fn io_error_source() {
    let err = GlyipError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "not found",
    ));
    assert!(err.source().is_some());
}

#[test]
fn non_io_error_no_source() {
    let err = GlyipError::Other("msg".to_string());
    assert!(err.source().is_none());
}

#[test]
fn from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let glyip_err: GlyipError = io_err.into();
    assert!(matches!(glyip_err, GlyipError::Io(_)));
}

#[test]
fn glyip_result_ok() {
    let result: GlyipResult<i32> = Ok(42);
    assert!(matches!(result, Ok(42)));
}

#[test]
fn glyip_result_err() {
    let result: GlyipResult<i32> = Err(GlyipError::Other("fail".to_string()));
    assert!(result.is_err());
}
