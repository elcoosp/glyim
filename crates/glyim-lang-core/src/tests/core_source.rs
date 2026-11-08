//! Tests for core source access.

use crate::{core_modules, core_source, core_source_all};

#[test]
fn core_source_returns_all_modules() {
    for name in core_modules() {
        assert!(
            core_source(name).is_some(),
            "core_source should return Some for module '{name}'"
        );
    }
}

#[test]
fn core_source_returns_none_for_unknown() {
    assert!(core_source("nonexistent_module").is_none());
}

#[test]
fn core_source_is_non_empty() {
    for name in core_modules() {
        let src = core_source(name).unwrap();
        assert!(
            !src.is_empty(),
            "core source for '{name}' should not be empty"
        );
    }
}

#[test]
fn core_source_all_contains_all_modules() {
    let all = core_source_all();
    for name in core_modules() {
        assert!(
            all.contains(&format!("// === module: {name} ===")),
            "core_source_all should contain module header for '{name}'"
        );
    }
}

#[test]
fn core_modules_count() {
    assert_eq!(core_modules().len(), 16, "should have 16 core modules");
}
