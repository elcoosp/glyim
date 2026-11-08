//! Tests for alloc source access.

use crate::{alloc_modules, alloc_source, alloc_source_all};

#[test]
fn alloc_source_returns_all_modules() {
    for name in alloc_modules() {
        assert!(
            alloc_source(name).is_some(),
            "alloc_source should return Some for module '{name}'"
        );
    }
}

#[test]
fn alloc_source_returns_none_for_unknown() {
    assert!(alloc_source("nonexistent_module").is_none());
}

#[test]
fn alloc_source_is_non_empty() {
    for name in alloc_modules() {
        let src = alloc_source(name).unwrap();
        assert!(
            !src.is_empty(),
            "alloc source for '{name}' should not be empty"
        );
    }
}

#[test]
fn alloc_source_all_contains_all_modules() {
    let all = alloc_source_all();
    for name in alloc_modules() {
        assert!(
            all.as_str().contains(&format!("// === module: {name} ===")),
            "alloc_source_all should contain module header for '{name}'"
        );
    }
}

#[test]
fn alloc_modules_count() {
    assert_eq!(alloc_modules().len(), 6, "should have 6 alloc modules");
}
