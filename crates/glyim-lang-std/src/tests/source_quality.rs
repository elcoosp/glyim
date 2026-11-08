//! Source quality verification for std library modules.

use crate::{std_module_count, std_modules, std_source, std_source_all};

#[test]
fn std_source_no_stub_macros() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        assert!(
            !src.contains("stub!"),
            "std module '{name}' should not contain stub!() macros"
        );
    }
}

#[test]
fn std_source_has_keyword_definitions() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        let has_definition = src.contains("fn ")
            || src.contains("struct ")
            || src.contains("enum ")
            || src.contains("trait ")
            || src.contains("impl ")
            || src.contains("type ")
            || src.contains("macro ");
        assert!(
            has_definition,
            "std module '{name}' should contain at least one definition (fn/struct/enum/trait/impl/type/macro)"
        );
    }
}

#[test]
fn std_source_all_is_non_empty() {
    let all = std_source_all();
    assert!(!all.is_empty(), "std_source_all should not be empty");
}

#[test]
fn std_source_all_modules_appear_exactly_once() {
    let all = std_source_all();
    for name in std_modules() {
        let header = format!("// === module: {name} ===");
        let count = all.matches(&header).count();
        assert_eq!(
            count, 1,
            "std_source_all should contain exactly one header for '{name}', found {count}"
        );
    }
}

#[test]
fn std_source_each_module_starts_with_doc_comment() {
    for name in std_modules() {
        let src = std_source(name).unwrap();
        assert!(
            src.starts_with("//!"),
            "std module '{name}' should start with a module-level doc comment (//!)"
        );
    }
}

#[test]
fn std_source_returns_none_for_empty_string() {
    assert!(std_source("").is_none());
}

#[test]
fn std_module_count_matches_modules_list() {
    assert_eq!(
        std_module_count(),
        std_modules().len(),
        "std_module_count should match std_modules().len()"
    );
}
