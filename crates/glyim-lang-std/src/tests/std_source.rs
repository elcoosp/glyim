//! Tests for std source access.

use crate::{std_module_count, std_modules, std_source, std_source_all};

#[test]
fn std_source_returns_all_modules() {
    for name in std_modules() {
        assert!(
            std_source(name).is_some(),
            "std_source should return Some for module '{}'",
            name
        );
    }
}

#[test]
fn std_source_returns_none_for_unknown() {
    // Only truly-unknown module names return None. The std source accessor
    // exposes every module the assembled stdlib depends on, including the
    // core extensions (str, slice, option, result, iter, ops, ...) and the
    // allocation layer (vec, boxed, rc, string, ...); the Script 6 extension
    // of std_source is intentional, so the old "core modules must not be in
    // std" assertion no longer holds.
    assert!(std_source("nonexistent_module").is_none());
    assert!(std_source("definitely_not_a_real_module_xyz").is_none());
    assert!(std_source("").is_none());
}

#[test]
fn std_source_is_non_empty() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        assert!(
            !src.is_empty(),
            "std source for '{}' should not be empty",
            name
        );
    }
}

#[test]
fn std_source_all_contains_all_modules() {
    let all = std_source_all();
    for name in std_modules() {
        assert!(
            all.contains(&format!("// === module: {} ===", name)),
            "std_source_all should contain module header for '{}'",
            name
        );
    }
}

#[test]
fn std_modules_count() {
    assert_eq!(std_module_count(), 8, "should have 8 std modules");
    assert_eq!(std_modules().len(), 8);
}

#[test]
fn std_source_has_doc_comments() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        assert!(
            src.contains("//!"),
            "std module '{}' should have module-level doc comments",
            name
        );
    }
}

#[test]
fn std_source_no_todo() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        assert!(
            !src.contains("todo!"),
            "std module '{}' should not contain todo!() macros",
            name
        );
    }
}
