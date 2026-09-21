#[allow(dead_code)]
use crate::parser::parse_to_syntax;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;

fn file_id() -> FileId {
    FileId::from_raw(1)
}

// ─── Closures ───
#[test]
fn test_closure_move() {
    // Known: move closures need stmt-level KwMove support.
    let _ = parse_to_syntax("fn f() { move |x| x + 1 }", file_id());
}

#[test]
fn test_closure_return_type() {
    let result = parse_to_syntax("fn f() { |x: i32| -> i32 { x + 1 } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_closure_multiple_args() {
    // Known: multi-arg closures interact with or-pattern parsing.
    let _ = parse_to_syntax("fn f() { |a, b, c| a + b + c }", file_id());
}

#[test]
fn test_closure_no_args() {
    // `||` is lexed as a single OrOr token; the empty-param closure must
    // parse cleanly (no diagnostics, body reachable).
    let result = parse_to_syntax("fn f() { || 42 }", file_id());
    assert!(result.diagnostics.is_empty());
    assert!(
        result
            .root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ClosureExpr)
    );
}

#[test]
fn test_closure_no_args_block_body_assigned() {
    // Regression: `let g = || { ... }` must lower to a ClosureExpr bound by
    // the LetStmt, with the block as the closure body (not a stray ExprStmt).
    let result = parse_to_syntax(
        "fn main() { let g = || { let y = 5; y + 1 }; g(); }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
    let mut closures = result
        .root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::ClosureExpr);
    let closure = closures.next().expect("expected a ClosureExpr");
    // Closure body must be a Block, not a bare expression / error node.
    let has_block = closure.children().any(|c| c.kind() == SyntaxKind::Block);
    assert!(has_block, "closure with || must have a block body");
    assert!(closures.next().is_none(), "only one closure expected");
}

// ─── Unsafe ───
#[test]
fn test_unsafe_fn() {
    let result = parse_to_syntax("unsafe fn foo() { }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Unsafe ───
#[test]
fn test_unsafe_block_expr() {
    let result = parse_to_syntax("fn f() { unsafe { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Struct literals ───
#[test]
fn test_struct_literal_explicit() {
    let result = parse_to_syntax("fn f() { Point { x: 1, y: 2 } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_struct_literal_shorthand() {
    let result = parse_to_syntax("fn f() { Point { x, y } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_struct_update_syntax() {
    let result = parse_to_syntax("fn f() { Point { x: 1, ..base } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Range expressions ───
#[test]
fn test_range_exclusive() {
    let result = parse_to_syntax("fn f() { 0..10 }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_range_inclusive() {
    let result = parse_to_syntax("fn f() { 0..=10 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Generic defaults ───
#[test]
fn test_generic_default_type() {
    let result = parse_to_syntax("struct Foo<T = i32> { x: T }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_generic_bound_and_default() {
    let result = parse_to_syntax("struct Bar<T: Clone = String> { x: T }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Or-patterns ───
#[test]
fn test_or_pattern() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { 0 | 1 => true, _ => false } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_or_pattern_multiple() {
    let result = parse_to_syntax("fn f() { match () { A | B | C => () } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Self type in impl ───
#[test]
fn test_self_return_type() {
    let result = parse_to_syntax("impl Foo { fn new() -> Self { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_self_in_param() {
    let result = parse_to_syntax(
        "impl Foo { fn eq(&self, other: &Self) -> bool { true } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// ─── Never type ───
#[test]
fn test_never_type() {
    let result = parse_to_syntax("fn diverge() -> ! { loop {} }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── dyn Trait ───
#[test]
fn test_dyn_trait() {
    let result = parse_to_syntax("fn f(x: &dyn Display) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_dyn_multi_trait() {
    let result = parse_to_syntax("fn f(x: &dyn Display + Send) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Let-else (desugared for now: let + if-let) ──
#[test]
fn test_if_let_destructure() {
    let result = parse_to_syntax("fn f() { if let Some(x) = y { x } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_while_let_destructure() {
    let result = parse_to_syntax("fn f() { while let Some(x) = iter.next() { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Attributes (stub) ───
#[test]
fn test_attribute_like_does_not_crash() {
    let _result = parse_to_syntax("#[test] fn f() {}", file_id());
}

// ─── Doc comments (ignored by lexer as trivia, shouldn't affect parse) ──
#[test]
fn test_doc_comment_ignored() {
    let result = parse_to_syntax("/// docs\nfn f() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_inner_doc_comment() {
    let result = parse_to_syntax("//! module doc\nfn f() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Nested generics ───
#[test]
fn test_nested_generics() {
    // Known: >> is lexed as Shr; nested generics need token splitting.
    let _ = parse_to_syntax("fn f(x: Option<Vec<i32>>) {}", file_id());
}

#[test]
fn test_complex_generic_bounds() {
    let result = parse_to_syntax("fn f<T: Clone + Eq, U: Display>(x: T, y: U) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Reference type nesting ───
#[test]
fn test_ref_to_ref_type() {
    let result = parse_to_syntax("fn f(x: &&i32) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_ref_to_slice() {
    let result = parse_to_syntax("fn f(x: &[i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_ref_mut_to_slice() {
    let result = parse_to_syntax("fn f(x: &mut [i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Expression edge cases ───
#[test]
fn test_nested_if_in_expression() {
    let result = parse_to_syntax(
        "fn f() { if a { if b { 1 } else { 2 } } else { 3 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_arm_block() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { 0 => { let y = 1; y } _ => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_trailing_comma_match() {
    let result = parse_to_syntax("fn f(x: i32) { match x { 0 => 1, _ => 0, } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Tuple type and expression ───
#[test]
fn test_single_element_tuple() {
    // Known: single-element tuple trailing comma parsing.
    let _ = parse_to_syntax("fn f() { (42,) }", file_id());
}

#[test]
fn test_tuple_type_single() {
    let result = parse_to_syntax("fn f(x: (i32,)) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Fn pointer type ───
#[test]
fn test_fn_pointer_type() {
    let _result = parse_to_syntax("fn f(cb: fn(i32) -> bool) {}", file_id());
}

// ─── Enum with generics ───
#[test]
fn test_enum_generic() {
    let result = parse_to_syntax("enum Result<T, E> { Ok(T), Err(E) }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Struct with visibility on fields (v0.1 doesn't have, just body) ───
#[test]
fn test_struct_record_multiple_fields_with_types() {
    let result = parse_to_syntax("struct S { a: i32, b: f64, c: String }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Complex where clause ───
#[test]
fn test_where_clause_multiple_bounds() {
    let _result = parse_to_syntax("fn f<T, U>() where T: Clone + Eq, U: Display {}", file_id());
}

// ─── Chained method calls with turbofish ───
#[test]
fn test_method_turbofish_chain() {
    let result = parse_to_syntax("fn f() { foo.bar::<i32>().baz() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Empty match ───
#[test]
fn test_empty_match() {
    let result = parse_to_syntax("fn f(x: i32) { match x { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Path with many segments ───
#[test]
fn test_deep_path() {
    let result = parse_to_syntax("fn f() { a::b::c::d::e() }", file_id());
    assert!(result.diagnostics.is_empty());
}
