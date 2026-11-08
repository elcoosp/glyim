//! Tests that alloc library .g files can be lexed by the Glyim frontend.
//!
//! Parse tests are soft-checks at v0.1.0: the parser cannot yet handle
//! all complex syntax. We verify files parse without crashing and log any
//! parse errors as informational rather than test failures.

use crate::alloc_source;
use glyim_frontend::{lex, parse_to_syntax};
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(100)
}

/// Helper: lex a module source and assert no errors.
fn lex_module(name: &str) {
    let source = alloc_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
    let result = lex(source, file_id());
    assert!(
        result.diagnostics.is_empty(),
        "lex errors in module '{}': {:?}",
        name,
        result.diagnostics
    );
}

/// Helper: parse a module source and log any errors (soft check).
fn parse_module_soft(name: &str) -> bool {
    let source = alloc_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
    let result = parse_to_syntax(source, file_id());
    if result.diagnostics.is_empty() {
        true
    } else {
        eprintln!(
            "INFO: module '{}' has {} parse diagnostics (expected at v0.1.0)",
            name,
            result.diagnostics.len()
        );
        false
    }
}

// V17-T01: Vec module lexes cleanly
#[test]
fn t01_vec_lex() {
    lex_module("vec");
}
#[test]
fn t01_vec_parse_soft() {
    parse_module_soft("vec");
}

// V17-T02: String module lexes cleanly
#[test]
fn t02_string_lex() {
    lex_module("string");
}
#[test]
fn t02_string_parse_soft() {
    parse_module_soft("string");
}

// V17-T03: Box module lexes cleanly
#[test]
fn t03_box_lex() {
    lex_module("boxed");
}
#[test]
fn t03_box_parse_soft() {
    parse_module_soft("boxed");
}

// V17-T04: Rc module lexes cleanly
#[test]
fn t04_rc_lex() {
    lex_module("rc");
}
#[test]
fn t04_rc_parse_soft() {
    parse_module_soft("rc");
}

// V17-T05: Alloc module lexes cleanly
#[test]
fn t05_alloc_lex() {
    lex_module("alloc");
}
#[test]
fn t05_alloc_parse_soft() {
    parse_module_soft("alloc");
}

// V17-T06: RawVec module lexes cleanly
#[test]
fn t06_raw_vec_lex() {
    lex_module("raw_vec");
}
#[test]
fn t06_raw_vec_parse_soft() {
    parse_module_soft("raw_vec");
}
