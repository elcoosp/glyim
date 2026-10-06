//! Wave 2 regression tests for FE-101/FE-103/FE-104/FE-107.

use crate::lexer::lex;
use crate::parser::parse_to_syntax;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn parse_ok(source: &str) -> Vec<glyim_diag::GlyimDiagnostic> {
    let file_id = FileId::from_raw(0);
    let lex_result = lex(source, file_id);
    let parse_result = parse_to_syntax(source, file_id);
    let mut diags = lex_result.diagnostics;
    diags.extend(parse_result.diagnostics);
    diags
}

fn has_kind(source: &str, kind: SyntaxKind) -> bool {
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    parse_result.root.descendants().any(|n| n.kind() == kind)
}

#[test]
fn t017_double_borrow_pattern_parses() {
    let src = "fn main() { let &&y = &r; }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T017: expected zero diagnostics, got {diags:?}"
    );
    assert!(
        has_kind(src, SyntaxKind::PatRef),
        "T017: expected a PatRef node"
    );
}

#[test]
fn t018_double_borrow_expr_parses() {
    let src = "fn main() { let r = &&x; }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T018: expected zero diagnostics, got {diags:?}"
    );
    assert!(
        has_kind(src, SyntaxKind::UnaryExpr),
        "T018: expected a UnaryExpr node"
    );
}

#[test]
fn t019_double_ref_type_with_mut_parses() {
    let src = "fn f(x: &&mut u8) {}";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T019: expected zero diagnostics, got {diags:?}"
    );
    assert!(
        has_kind(src, SyntaxKind::RefType),
        "T019: expected RefType nodes"
    );
}

#[test]
fn t019_double_ref_type_with_lifetime_parses() {
    let src = "fn f<'a>(x: &&'a u8) {}";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T019: expected zero diagnostics, got {diags:?}"
    );
}

#[test]
fn t022_negative_literal_in_pattern_parses() {
    let src = "fn main() { match x { -1 => 1, _ => 0 } }";
    let diags = parse_ok(src);
    assert!(
        diags.is_empty(),
        "T022: expected zero diagnostics, got {diags:?}"
    );
}
