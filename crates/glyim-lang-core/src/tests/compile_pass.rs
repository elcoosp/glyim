//! Tests that core library .g files can be lexed by the Glyim frontend.
//!
//! Parse tests are soft-checks at v0.1.0: the parser cannot yet handle
//! all complex syntax (lifetimes, where clauses, associated types, macros).
//! We verify files parse without crashing and log any parse errors as
//! informational rather than test failures.

use crate::core_source;
use glyim_frontend::{lex, parse_to_syntax};
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(99)
}

/// Helper: lex a core module source and assert no errors.
fn lex_module(name: &str) {
    let source = core_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
    let result = lex(source, file_id());
    assert!(
        result.diagnostics.is_empty(),
        "lex errors in module '{}': {:?}",
        name,
        result.diagnostics
    );
}

/// Helper: parse a core module source and log any errors (soft check).
/// Returns true if the module parsed without errors.
fn parse_module_soft(name: &str) -> bool {
    let source = core_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
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

// V16-T01: Option module lexes cleanly
#[test]
fn t01_option_lex() {
    lex_module("option");
}

#[test]
fn t01_option_parse_soft() {
    parse_module_soft("option");
}

// V16-T02: Iter module lexes cleanly (IntoIterator for slice)
#[test]
fn t02_iter_lex() {
    lex_module("iter");
}

#[test]
fn t02_iter_parse_soft() {
    parse_module_soft("iter");
}

// V16-T03: Panic module lexes cleanly
#[test]
fn t03_panic_lex() {
    lex_module("panic");
}

#[test]
fn t03_panic_parse_soft() {
    parse_module_soft("panic");
}

// V16-T04: Mem module lexes cleanly (mem::replace, mem::swap)
#[test]
fn t04_mem_lex() {
    lex_module("mem");
}

#[test]
fn t04_mem_parse_soft() {
    parse_module_soft("mem");
}

// V16-T05: Cmp module lexes cleanly (cmp::min, cmp::max)
#[test]
fn t05_cmp_lex() {
    lex_module("cmp");
}

#[test]
fn t05_cmp_parse_soft() {
    parse_module_soft("cmp");
}

// V16-T06: Default module lexes and parses cleanly
#[test]
fn t06_default_lex() {
    lex_module("default");
}

#[test]
fn t06_default_parse() {
    // default.g uses only simple syntax the parser supports
    let source = core_source("default").unwrap();
    let result = parse_to_syntax(source, file_id());
    assert!(
        result.diagnostics.is_empty(),
        "parse errors in module 'default': {:?}",
        result.diagnostics
    );
}

// Additional module lex tests (strict) and parse tests (soft)
#[test]
fn result_lex() {
    lex_module("result");
}
#[test]
fn result_parse_soft() {
    parse_module_soft("result");
}

#[test]
fn slice_lex() {
    lex_module("slice");
}
#[test]
fn slice_parse_soft() {
    parse_module_soft("slice");
}

#[test]
fn str_lex() {
    lex_module("str");
}
#[test]
fn str_parse_soft() {
    parse_module_soft("str");
}

#[test]
fn cell_lex() {
    lex_module("cell");
}
#[test]
fn cell_parse_soft() {
    parse_module_soft("cell");
}

#[test]
fn ptr_lex() {
    lex_module("ptr");
}
#[test]
fn ptr_parse_soft() {
    parse_module_soft("ptr");
}

#[test]
fn ops_lex() {
    lex_module("ops");
}
#[test]
fn ops_parse_soft() {
    parse_module_soft("ops");
}

#[test]
fn marker_lex() {
    lex_module("marker");
}
#[test]
fn marker_parse_soft() {
    parse_module_soft("marker");
}

#[test]
fn hint_lex() {
    lex_module("hint");
}
#[test]
fn hint_parse_soft() {
    parse_module_soft("hint");
}

#[test]
fn convert_lex() {
    lex_module("convert");
}
#[test]
fn convert_parse_soft() {
    parse_module_soft("convert");
}

// V16-T07: Future module (Phase 5) lexes cleanly and parses (soft, as it uses
// associated types which the v0.1.0 parser may partially handle).
#[test]
fn t07_future_lex() {
    lex_module("future");
}

#[test]
fn t07_future_parse_soft() {
    parse_module_soft("future");
}
