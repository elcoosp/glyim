//! Source quality verification for alloc library modules.

use crate::{alloc_modules, alloc_source, alloc_source_all};

#[test]
fn alloc_source_no_stub_macros() {
    for name in alloc_modules() {
        let src = alloc_source(name).unwrap();
        assert!(
            !src.contains("stub!"),
            "alloc module '{name}' should not contain stub!() macros"
        );
    }
}

#[test]
fn alloc_source_no_todo_macros() {
    for name in alloc_modules() {
        let src = alloc_source(name).unwrap();
        assert!(
            !src.contains("todo!"),
            "alloc module '{name}' should not contain todo!() macros"
        );
    }
}

#[test]
fn alloc_source_has_keyword_definitions() {
    for name in alloc_modules() {
        let src = alloc_source(name).unwrap();
        let has_definition = src.contains("fn ")
            || src.contains("struct ")
            || src.contains("enum ")
            || src.contains("trait ")
            || src.contains("impl ")
            || src.contains("type ");
        assert!(
            has_definition,
            "alloc module '{name}' should contain at least one definition (fn/struct/enum/trait/impl/type)"
        );
    }
}

#[test]
fn alloc_source_all_is_non_empty() {
    let all = alloc_source_all();
    assert!(!all.is_empty(), "alloc_source_all should not be empty");
}

#[test]
fn alloc_source_all_has_all_module_headers() {
    let all = alloc_source_all();
    for name in alloc_modules() {
        let header = format!("// === module: {name} ===");
        assert!(
            all.contains(&header),
            "alloc_source_all should contain header for '{name}'"
        );
    }
}

#[test]
fn alloc_source_all_modules_appear_exactly_once() {
    let all = alloc_source_all();
    for name in alloc_modules() {
        let header = format!("// === module: {name} ===");
        let count = all.matches(&header).count();
        assert_eq!(
            count, 1,
            "alloc_source_all should contain exactly one header for '{name}', found {count}"
        );
    }
}

#[test]
fn alloc_source_each_module_starts_with_doc_comment() {
    for name in alloc_modules() {
        let src = alloc_source(name).unwrap();
        assert!(
            src.starts_with("//!"),
            "alloc module '{name}' should start with a module-level doc comment (//!)"
        );
    }
}
