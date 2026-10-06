//! Wave 2 regression tests for FE-105/FE-106.

use crate::lexer::lex;
use crate::parser::parse_to_syntax;
use glyim_span::FileId;

fn parse_ok(source: &str) -> Vec<glyim_diag::GlyimDiagnostic> {
    let file_id = FileId::from_raw(0);
    let lex_result = lex(source, file_id);
    let parse_result = parse_to_syntax(source, file_id);
    let mut diags = lex_result.diagnostics;
    diags.extend(parse_result.diagnostics);
    diags
}

#[test]
fn t020_generic_args_on_first_path_segment() {
    let src = "fn main() { let v = Vec<u8>::new(); }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T020: expected zero diagnostics, got {diags:?}"
    );
}

#[test]
fn t020_combined_with_turbofish() {
    // Turbofish form should keep parsing.
    let src = "fn main() { let v = Vec::<u8>::new(); }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T020: expected zero diagnostics (turbofish), got {diags:?}"
    );
}

#[test]
fn t021_bit_or_and_precedence() {
    // `4 | 1 & 3` parses without diagnostics. Semantics verified
    // end-to-end via the compiler path in glyim-test.
    let src = "fn main() -> i32 { 4 | 1 & 3 }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T021: expected zero diagnostics, got {diags:?}"
    );
}

#[test]
fn t021_bit_and_shift_precedence() {
    let src = "fn main() -> i32 { 2 & 1 << 1 }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T021: expected zero diagnostics, got {diags:?}"
    );
}

#[test]
fn t021_bit_xor_and_precedence() {
    let src = "fn main() -> i32 { 6 ^ 3 & 1 }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T021: expected zero diagnostics, got {diags:?}"
    );
}
