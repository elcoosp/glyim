//! Source quality verification for core library modules.

use crate::{core_modules, core_source, core_source_all};

#[test]
fn core_source_no_stub_macros() {
    for name in core_modules() {
        let src = core_source(name).unwrap();
        assert!(
            !src.contains("stub!"),
            "core module '{name}' should not contain stub!() macros"
        );
    }
}

#[test]
fn core_source_no_todo_macros() {
    for name in core_modules() {
        let src = core_source(name).unwrap();
        assert!(
            !src.contains("todo!"),
            "core module '{name}' should not contain todo!() macros"
        );
    }
}

#[test]
fn core_source_has_keyword_definitions() {
    for name in core_modules() {
        let src = core_source(name).unwrap();
        let has_definition = src.contains("fn ")
            || src.contains("struct ")
            || src.contains("enum ")
            || src.contains("trait ")
            || src.contains("impl ")
            || src.contains("type ")
            || src.contains("macro ");
        assert!(
            has_definition,
            "core module '{name}' should contain at least one definition (fn/struct/enum/trait/impl/type/macro)"
        );
    }
}

#[test]
fn core_source_all_is_non_empty() {
    let all = core_source_all();
    assert!(!all.is_empty(), "core_source_all should not be empty");
}

#[test]
fn core_source_all_modules_appear_exactly_once() {
    let all = core_source_all();
    for name in core_modules() {
        let header = format!("// === module: {name} ===");
        let count = all.matches(&header).count();
        assert_eq!(
            count, 1,
            "core_source_all should contain exactly one header for '{name}', found {count}"
        );
    }
}

#[test]
fn core_source_each_module_starts_with_doc_comment() {
    for name in core_modules() {
        let src = core_source(name).unwrap();
        assert!(
            src.starts_with("//!"),
            "core module '{name}' should start with a module-level doc comment (//!)"
        );
    }
}

#[test]
fn core_source_returns_none_for_empty_string() {
    assert!(core_source("").is_none());
}

#[test]
fn core_source_returns_none_for_core_keyword() {
    assert!(core_source("core").is_none());
}
