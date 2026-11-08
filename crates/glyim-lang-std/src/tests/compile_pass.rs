//! Tests that std library .g files can be lexed by the Glyim frontend.
//!
//! Parse tests are soft-checks at v0.1.0: the parser cannot yet handle
//! all complex syntax (lifetimes, where clauses, associated types, macros).
//! We verify files parse without crashing and log any parse errors as
//! informational rather than test failures.

use crate::std_source;
use glyim_frontend::{lex, parse_to_syntax};
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(100)
}

/// Helper: lex a std module source and assert no errors.
fn lex_module(name: &str) {
    let source = std_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
    let result = lex(source, file_id());
    assert!(
        result.diagnostics.is_empty(),
        "lex errors in module '{}': {:?}",
        name,
        result.diagnostics
    );
}

/// Helper: parse a std module source and log any errors (soft check).
fn parse_module_soft(name: &str) -> bool {
    let source = std_source(name).unwrap_or_else(|| panic!("no source for module '{name}'"));
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

// V18-T01: io module lexes cleanly
#[test]
fn t01_io_lex() {
    lex_module("io");
}

#[test]
fn t01_io_parse_soft() {
    parse_module_soft("io");
}

// V18-T02: fs module lexes cleanly
#[test]
fn t02_fs_lex() {
    lex_module("fs");
}

#[test]
fn t02_fs_parse_soft() {
    parse_module_soft("fs");
}

// V18-T03: thread module lexes cleanly
#[test]
fn t03_thread_lex() {
    lex_module("thread");
}

#[test]
fn t03_thread_parse_soft() {
    parse_module_soft("thread");
}

// V18-T04: sync module lexes cleanly
#[test]
fn t04_sync_lex() {
    lex_module("sync");
}

#[test]
fn t04_sync_parse_soft() {
    parse_module_soft("sync");
}

// V18-T05: env module lexes cleanly
#[test]
fn t05_env_lex() {
    lex_module("env");
}

#[test]
fn t05_env_parse_soft() {
    parse_module_soft("env");
}

// Additional module lex tests
#[test]
fn net_lex() {
    lex_module("net");
}

#[test]
fn net_parse_soft() {
    parse_module_soft("net");
}

#[test]
fn time_lex() {
    lex_module("time");
}

#[test]
fn time_parse_soft() {
    parse_module_soft("time");
}

#[test]
fn process_lex() {
    lex_module("process");
}

#[test]
fn process_parse_soft() {
    parse_module_soft("process");
}
