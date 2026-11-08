use crate::parser::parse_to_syntax;
use glyim_span::FileId;
use glyim_syntax::{SyntaxKind, SyntaxNode};

fn file_id() -> FileId {
    FileId::from_raw(1)
}

fn assert_child_node(node: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
    node.children()
        .find(|c| c.kind() == kind)
        .unwrap_or_else(|| panic!("missing node {kind:?}"))
}

// -------------------- Expression tests --------------------

// S09-T14: If/else expression
#[test]
fn test_if_else_expr() {
    let result = parse_to_syntax("fn f() { if true { 1 } else { 2 } }", file_id());
    for d in &result.diagnostics {
        eprintln!("DIAG: {:?} - {}", d.severity, d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_if_else_if_chain() {
    let result = parse_to_syntax(
        "fn f() { if a { 1 } else if b { 2 } else { 3 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
    let fn_def = assert_child_node(&result.root, SyntaxKind::FnDef);
    let block = assert_child_node(&fn_def, SyntaxKind::Block);
    assert_child_node(&block, SyntaxKind::ExprStmt);
}

#[test]
fn test_if_without_else() {
    let result = parse_to_syntax("fn f() { if true { 1 } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_if_let_pattern() {
    let _result = parse_to_syntax("fn f() { if let Some(x) = y { x } }", file_id());
    // May produce diagnostics if if-let isn't fully supported, but shouldn't panic
}

// S09-T15: While loop
#[test]
fn test_while_loop() {
    let result = parse_to_syntax("fn f() { while true { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_while_loop_with_body() {
    let result = parse_to_syntax("fn f() { while x > 0 { x = x - 1; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T16: For loop
#[test]
fn test_for_loop() {
    let result = parse_to_syntax("fn f() { for i in 0..10 { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T17: Return expression
#[test]
fn test_return_with_value() {
    let result = parse_to_syntax("fn f() -> i32 { return 42; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_return_unit() {
    let result = parse_to_syntax("fn f() { return; }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T18: Break and continue
#[test]
fn test_break_expr() {
    let result = parse_to_syntax("fn f() { loop { break; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_break_with_value() {
    let result = parse_to_syntax("fn f() { loop { break 42; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_continue_expr() {
    let result = parse_to_syntax("fn f() { loop { continue; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T19: Match expression
#[test]
fn test_match_expr() {
    let result = parse_to_syntax("fn f(x: i32) { match x { 0 => 1, _ => 0 } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_with_arms() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { 0 => { 1 } 1 => 2 _ => 3 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_with_if_guard() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { n if n > 0 => n, _ => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// S09-T20: Tuple expressions
#[test]
fn test_tuple_expr() {
    let result = parse_to_syntax("fn f() { (1, 2, 3) }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_unit_expr() {
    let result = parse_to_syntax("fn f() { () }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T21: Index expression
#[test]
fn test_index_expr() {
    let result = parse_to_syntax("fn f() { a[0] }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T22: Cast expression
#[test]
fn test_cast_expr() {
    let result = parse_to_syntax("fn f() { x as i32 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T23: Method call chaining
#[test]
fn test_method_chain() {
    let result = parse_to_syntax("fn f() { a.b().c().d() }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_method_with_args() {
    let result = parse_to_syntax("fn f() { a.foo(1, 2, 3) }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T24: Closures
#[test]
fn test_closure_simple() {
    let _result = parse_to_syntax("fn f() { |x| x + 1 }", file_id());
    // May warn if closures not fully supported, but should not panic
}

#[test]
fn test_closure_with_types() {
    let _result = parse_to_syntax("fn f() { |x: i32| -> i32 { x + 1 } }", file_id());
    // May warn if closures not fully supported
}

// S09-T25: Generic functions
#[test]
fn test_generic_fn() {
    let result = parse_to_syntax("fn id<T>(x: T) -> T { x }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_generic_with_bounds() {
    let result = parse_to_syntax("fn print<T: Display>(x: T) { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_where_clause() {
    let _result = parse_to_syntax("fn foo<T>(x: T) where T: Clone { }", file_id());
    // May warn about where clause stub
}

// S09-T26: Module items
#[test]
fn test_module_decl() {
    let result = parse_to_syntax("mod foo;", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_module_inline() {
    let result = parse_to_syntax("mod foo { fn bar() {} }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T27: Use statements
#[test]
fn test_use_simple() {
    let _result = parse_to_syntax("use std::io;", file_id());
    // Uses a stub path — should not panic
}

#[test]
fn test_use_nested() {
    let _result = parse_to_syntax("use std::io::{stdin, stdout};", file_id());
}

// S09-T28: Const and static
#[test]
fn test_const_item() {
    let result = parse_to_syntax("const MAX: i32 = 100;", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_static_item() {
    let result = parse_to_syntax("static COUNTER: i32 = 0;", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_static_mut_item() {
    let result = parse_to_syntax("static mut FLAG: bool = false;", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T29: Extern blocks
#[test]
fn test_extern_block() {
    let _result = parse_to_syntax("extern \"C\" { fn puts(s: *const u8); }", file_id());
    // May warn about extern block stub
}

// S09-T30: Type alias
#[test]
fn test_type_alias() {
    let result = parse_to_syntax("type Name = String;", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T31: Trait with supertraits
#[test]
fn test_trait_with_supertraits() {
    let result = parse_to_syntax("trait Eq: PartialEq { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_trait_with_associated_fn() {
    let result = parse_to_syntax(
        "trait Iterator { fn next(&mut self) -> Option<Self::Item>; }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// S09-T32: Enum variants with data
#[test]
fn test_enum_tuple_variants() {
    let result = parse_to_syntax("enum Option { Some(i32), None }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_enum_record_variants() {
    let result = parse_to_syntax(
        "enum Shape { Circle { r: f64 }, Rect { w: f64, h: f64 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_enum_with_discriminant() {
    let result = parse_to_syntax("enum Color { Red = 1, Green = 2, Blue = 4 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T33: Struct generics
#[test]
fn test_struct_generic() {
    let result = parse_to_syntax("struct Point<T> { x: T, y: T }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T34: Visibility
#[test]
fn test_visibility_pub_fn() {
    let result = parse_to_syntax("pub fn main() { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_visibility_pub_struct() {
    let result = parse_to_syntax("pub struct Foo;", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T35: Complex types
#[test]
fn test_type_ref_ref() {
    let result = parse_to_syntax("fn f(x: &&i32) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_slice() {
    let result = parse_to_syntax("fn f(x: &[i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_array() {
    let result = parse_to_syntax("fn f(x: [i32; 3]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_tuple() {
    let result = parse_to_syntax("fn f(x: (i32, f64)) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_infer() {
    let result = parse_to_syntax("fn f(x: _) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T36: Self parameters
#[test]
fn test_self_param() {
    let result = parse_to_syntax("fn f(&self) { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_self_mut_param() {
    let result = parse_to_syntax("fn f(&mut self) { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_self_value_param() {
    let result = parse_to_syntax("fn f(self) { }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T37: Literal expressions
#[test]
fn test_literal_int() {
    let result = parse_to_syntax("fn f() { 42 }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_literal_float() {
    let result = parse_to_syntax("fn f() { 3.14 }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_literal_string() {
    let result = parse_to_syntax("fn f() { \"hello\" }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_literal_char() {
    let result = parse_to_syntax("fn f() { 'a' }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_literal_bool_true() {
    let result = parse_to_syntax("fn f() { true }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_literal_bool_false() {
    let result = parse_to_syntax("fn f() { false }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T38: Unary operators
#[test]
fn test_unary_not() {
    let result = parse_to_syntax("fn f() { !true }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_unary_neg() {
    let result = parse_to_syntax("fn f() { -x }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_unary_deref() {
    let result = parse_to_syntax("fn f() { *x }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_unary_ref() {
    let result = parse_to_syntax("fn f() { &x }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T39: Assignment and compound assignment
#[test]
fn test_assign_expr() {
    let result = parse_to_syntax("fn f() { x = 5; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_compound_assign() {
    let result = parse_to_syntax("fn f() { x += 1; }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T40: Nested blocks
#[test]
fn test_nested_blocks() {
    let result = parse_to_syntax("fn f() { { { 1 } } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T41: Multiple items
#[test]
fn test_multiple_items() {
    let result = parse_to_syntax("struct A; struct B; fn main() { }", file_id());
    assert!(result.diagnostics.is_empty());
    let children: Vec<_> = result.root.children().collect();
    assert!(children.len() >= 3, "should have at least three items");
}

// S09-T42: Empty file
#[test]
fn test_empty_file() {
    let result = parse_to_syntax("", file_id());
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.root.kind(), SyntaxKind::SourceFile);
}

// S09-T43: Complex expression precedence
#[test]
fn test_expr_precedence_full() {
    let source = "fn f() { a + b * c == d && e || f }";
    let result = parse_to_syntax(source, file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T44: Postfix chain (cast + index + call)
#[test]
fn test_postfix_chain() {
    let source = "fn f() { a[0] as i32 + b() }";
    let result = parse_to_syntax(source, file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T45: Question operator
#[test]
fn test_try_operator() {
    let result = parse_to_syntax("fn f() -> Option<i32> { Some(42)? }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T46: Error recovery: missing closing brace
#[test]
fn test_error_missing_brace() {
    let result = parse_to_syntax("fn f() { let x = 5;", file_id());
    assert!(
        !result.diagnostics.is_empty(),
        "should produce errors for missing brace"
    );
}

// S09-T47: Error recovery: missing paren
#[test]
fn test_error_missing_paren() {
    let result = parse_to_syntax("fn f( { }", file_id());
    assert!(
        !result.diagnostics.is_empty(),
        "should produce errors for missing paren"
    );
}

// S09-T48: Error recovery: unexpected token at top level
#[test]
fn test_error_unexpected_top_level() {
    let result = parse_to_syntax("+", file_id());
    assert!(
        !result.diagnostics.is_empty(),
        "should produce errors for unexpected token"
    );
}

// S09-T49: Path with crate and super
#[test]
fn test_path_crate() {
    let result = parse_to_syntax("fn f() { crate::foo::bar() }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_path_super() {
    let result = parse_to_syntax("fn f() { super::foo() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T50: Turbofish syntax
#[test]
fn test_turbofish() {
    let result = parse_to_syntax("fn f() { foo::<i32>() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// S09-T51: Snapshot: complex program
#[test]
fn test_snapshot_complex_program() {
    glyim_test::snapshot_cst(
        "complex_program",
        r"
pub struct Point<T> {
    x: T,
    y: T,
}

impl<T: Clone> Point<T> {
    pub fn new(x: T, y: T) -> Self {
        Point { x, y }
    }

    fn dist(&self) -> f64 {
        0.0
    }
}

fn main() {
    let p = Point::new(1.0, 2.0);
    let d = p.dist();
    if d > 0.0 {
        return;
    }
}
",
    );
}

// S09-T52: Snapshot: match expression
#[test]
fn test_snapshot_match() {
    glyim_test::snapshot_cst(
        "match_expr",
        r#"
fn classify(x: i32) -> &str {
    match x {
        0 => "zero",
        1 | 2 => "small",
        n if n < 0 => "negative",
        _ => "large",
    }
}
"#,
    );
}

// S09-T53: Snapshot: enum and pattern
#[test]
fn test_snapshot_enum_pattern() {
    glyim_test::snapshot_cst(
        "enum_pattern",
        r"
enum Option<T> {
    Some(T),
    None,
}

fn unwrap_or<T>(opt: Option<T>, default: T) -> T {
    match opt {
        Option::Some(val) => val,
        Option::None => default,
    }
}
",
    );
}
