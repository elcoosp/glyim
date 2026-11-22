use crate::parse_to_syntax;
use glyim_span::FileId;
use glyim_syntax::SyntaxKind;
use glyim_test::assert_no_errors;

fn test_parse(source: &str) {
    let file_id = FileId::from_usize(1);
    let result = parse_to_syntax(source, file_id);
    assert_no_errors(&result.diagnostics);
    // Root should not be an error node
    assert_ne!(result.root.kind(), SyntaxKind::Error);
}

#[test]
fn parse_empty_source() {
    test_parse("");
}

#[test]
fn parse_simple_fn() {
    test_parse("fn main() { }");
}

#[test]
fn parse_fn_with_params() {
    test_parse("fn add(x: i32, y: i32) -> i32 { x + y }");
}

#[test]
fn parse_struct_def() {
    test_parse("struct Point { x: f64, y: f64 }");
}

#[test]
fn parse_enum_def() {
    test_parse("enum Option<T> { Some(T), None }");
}

#[test]
fn parse_impl_block() {
    test_parse("impl<T> Option<T> { fn is_some(&self) -> bool { true } }");
}

#[test]
fn parse_trait_def() {
    test_parse("trait Display { fn fmt(&self) -> String; }");
}

#[test]
fn parse_use_decl() {
    test_parse("use std::collections::HashMap;");
}

#[test]
fn parse_expression_binary() {
    test_parse("fn foo() { let x = 1 + 2 * 3; }");
}

#[test]
fn parse_if_else() {
    test_parse("fn foo() { if x { 1 } else { 2 } }");
}

#[test]
fn parse_while_loop() {
    test_parse("fn foo() { while i < 10 { i = i + 1; } }");
}

#[test]
fn parse_for_loop() {
    test_parse("fn foo() { for item in list { process(item); } }");
}

#[test]
fn parse_match() {
    test_parse("fn foo(x: Option<i32>) { match x { Some(v) => v, None => 0 } }");
}

#[test]
fn parse_block_as_expression() {
    test_parse("fn foo() { let y = { let x = 5; x + 1 }; }");
}

#[test]
fn parse_closure() {
    test_parse("fn foo() { let add = |a, b| a + b; }");
}

#[test]
fn parse_macro_invocation() {
    test_parse("fn main() { println!(\"Hello\"); }");
}

#[test]
fn parse_type_annotations() {
    test_parse("fn foo() { let x: i32 = 42; }");
}

#[test]
fn parse_reference_types() {
    test_parse("fn foo(x: &mut i32) { *x = 0; }");
}

#[test]
fn parse_array_and_slice() {
    test_parse("fn foo() { let arr: [i32; 5] = [1,2,3,4,5]; let slice: &[i32] = &arr; }");
}

#[test]
fn parse_tuple() {
    test_parse("fn foo() { let t: (i32, bool) = (42, true); }");
}

#[test]
fn parse_generic_fn() {
    test_parse("fn identity<T>(x: T) -> T { x }");
}
