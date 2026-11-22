use crate::parse_to_syntax;
use glyim_span::FileId;
use glyim_test::snapshot_cst;

#[test]
fn test_index_expr() {
    let source = "fn main() { let x = arr[0]; }";
    let parse_result = parse_to_syntax(source, FileId::from_usize(1));
    assert!(parse_result.diagnostics.is_empty());
    snapshot_cst("index_expr", source);
}

#[test]
fn test_or_pattern() {
    let source = "fn main() { match x { 0 | 1 => {} } }";
    let parse_result = parse_to_syntax(source, FileId::from_usize(1));
    assert!(parse_result.diagnostics.is_empty());
    snapshot_cst("or_pattern", source);
}

#[test]
fn test_range_pattern_inclusive() {
    let source = "fn main() { match x { 0..=9 => {} } }";
    let parse_result = parse_to_syntax(source, FileId::from_usize(1));
    assert!(parse_result.diagnostics.is_empty());
    snapshot_cst("range_pattern_inclusive", source);
}

#[test]
fn test_slice_pattern() {
    let source = "fn main() { match arr { [a, b, .., c] => {} } }";
    let parse_result = parse_to_syntax(source, FileId::from_usize(1));
    assert!(parse_result.diagnostics.is_empty());
    snapshot_cst("slice_pattern", source);
}
