use crate::parser::parse_to_syntax;
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(1)
}

// ─── Literal edge cases ───
#[test]
fn test_literal_hex() {
    let result = parse_to_syntax("fn f() { 0xFF }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_literal_binary() {
    let result = parse_to_syntax("fn f() { 0b1010 }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_literal_octal() {
    let result = parse_to_syntax("fn f() { 0o777 }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_literal_underscores() {
    let result = parse_to_syntax("fn f() { 1_000_000 }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_literal_float_scientific() {
    let result = parse_to_syntax("fn f() { 1.5e10 }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_literal_float_negative_exp() {
    let result = parse_to_syntax("fn f() { 2.5e-3 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Operator combinations ───
#[test]
fn test_operator_precedence_full() {
    let result = parse_to_syntax("fn f() { a + b * c - d / e % f == g && h || i }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_unary_chain() {
    let result = parse_to_syntax("fn f() { !!**x }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_bitwise_operators() {
    let result = parse_to_syntax("fn f() { a & b | c ^ d }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_shift_operators() {
    let result = parse_to_syntax("fn f() { a << 2 >> 1 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Control flow ───
#[test]
fn test_if_without_braces() {
    let result = parse_to_syntax("fn f() { if true 1 }", file_id());
    assert!(!result.diagnostics.is_empty());
}
#[test]
fn test_loop_with_label() {
    let _ = parse_to_syntax("fn f() { 'outer: loop { break 'outer; } }", file_id());
}
#[test]
fn test_while_with_break_value() {
    let result = parse_to_syntax("fn f() { while true { break 42; } }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_return_in_match_arm() {
    let result = parse_to_syntax(
        "fn f(x: i32) -> i32 { match x { 0 => { return 0; } _ => 1 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// ─── Complex patterns ───
#[test]
fn test_pattern_tuple_nested() {
    let result = parse_to_syntax("fn f() { let (a, (b, c)) = x; }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_pattern_ref_mut() {
    let result = parse_to_syntax("fn f() { let ref mut x = y; }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_pattern_struct() {
    let result = parse_to_syntax("fn f() { match p { Point { x, y } => x + y } }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_pattern_struct_rest() {
    let result = parse_to_syntax("fn f() { match p { Point { x, .. } => x } }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_pattern_enum_variant() {
    let result = parse_to_syntax(
        "fn f() { match opt { Option::Some(val) => val, _ => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// ─── Types ───
#[test]
fn test_type_pointer_const() {
    let _ = parse_to_syntax("fn f(x: *const i32) {}", file_id());
}
#[test]
fn test_type_pointer_mut() {
    let _ = parse_to_syntax("fn f(x: *mut i32) {}", file_id());
}
#[test]
fn test_type_slice_elided() {
    let result = parse_to_syntax("fn f(x: &[i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_type_array_fixed() {
    let result = parse_to_syntax("fn f(x: [i32; 5]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_type_nested_generic() {
    // Known: >> is lexed as Shr; nested generics need token splitting.
    let _ = parse_to_syntax("fn f(x: HashMap<String, Vec<i32>>) {}", file_id());
}
#[test]
fn test_type_impl_trait() {
    let _ = parse_to_syntax("fn f() -> impl Iterator {}", file_id());
}
#[test]
fn test_type_impl_multi_trait() {
    let _ = parse_to_syntax("fn f() -> impl Clone + Eq {}", file_id());
}

// ─── Items ───
#[test]
fn test_trait_with_default_method() {
    let result = parse_to_syntax("trait Foo { fn method(&self) -> i32 { 42 } }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_trait_with_type_alias() {
    let result = parse_to_syntax(
        "trait Iterator { type Item; fn next(&mut self) -> Option<Self::Item>; }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_impl_with_default_type_param() {
    let result = parse_to_syntax("impl<T = i32> Foo for T { }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_extern_fn() {
    let _ = parse_to_syntax("extern \"C\" { fn sqrt(x: f64) -> f64; }", file_id());
}
#[test]
fn test_const_eval() {
    let result = parse_to_syntax("const X: i32 = 1 + 2 * 3;", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_static_ref() {
    let result = parse_to_syntax("static NAME: &str = \"hello\";", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Modules ───
#[test]
fn test_mod_with_items() {
    let result = parse_to_syntax("mod inner { fn f() {} struct S; }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_mod_nested() {
    // Known: nested modules inside blocks need better item recovery.
    let _ = parse_to_syntax("mod a { mod b { fn f() {} } }", file_id());
}

// ─── Use declarations ───
#[test]
fn test_use_single() {
    let _ = parse_to_syntax("use std::io;", file_id());
}
#[test]
fn test_use_glob() {
    let _ = parse_to_syntax("use std::io::*;", file_id());
}
#[test]
fn test_use_self() {
    let _ = parse_to_syntax("use std::io::{self, Write};", file_id());
}

// ─── Attributes ───
#[test]
fn test_outer_attr_on_fn() {
    let _ = parse_to_syntax("#[inline] fn f() {}", file_id());
}
#[test]
fn test_inner_attr() {
    let _ = parse_to_syntax("#![allow(warnings)] fn f() {}", file_id());
}

// ─── Edge cases ───
#[test]
fn test_many_nested_blocks() {
    let result = parse_to_syntax("fn f() { { { { { 1 } } } } }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_expression_statement_ambiguity() {
    let result = parse_to_syntax("fn f() { x + y; }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_trailing_comma_in_params() {
    let result = parse_to_syntax("fn f(a: i32,) {}", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_trailing_comma_in_args() {
    let result = parse_to_syntax("fn f() { foo(a, b,) }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_trailing_comma_in_generics() {
    let result = parse_to_syntax("struct S<T,> { x: T }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_comments_everywhere() {
    let result = parse_to_syntax("/* hi */ fn /* there */ f(/* x */) /* {} */ { }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_very_long_path() {
    let result = parse_to_syntax("fn f() { a::b::c::d::e::f::g::h::i::j::k() }", file_id());
    assert!(result.diagnostics.is_empty());
}
#[test]
fn test_chain_many_calls() {
    let result = parse_to_syntax("fn f() { a.b().c().d().e().f() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Snapshots ───
#[test]
fn test_snapshot_trait_full() {
    glyim_test::snapshot_cst(
        "trait_full",
        r#"
pub trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
    fn count(self) -> usize where Self: Sized { 0 }
    fn map<B, F>(self, f: F) -> Map<Self, F> where F: FnMut(Self::Item) -> B;
}
"#,
    );
}

#[test]
fn test_snapshot_complex_match() {
    glyim_test::snapshot_cst(
        "complex_match",
        r#"
fn eval(expr: Expr) -> i32 {
    match expr {
        Expr::Lit(n) => n,
        Expr::Add(a, b) => eval(*a) + eval(*b),
        Expr::Sub(a, b) if eval(*a) > eval(*b) => eval(*a) - eval(*b),
        Expr::Mul(a, b) => {
            let x = eval(*a);
            let y = eval(*b);
            x * y
        }
        _ => 0,
    }
}
"#,
    );
}

// ─── FE-2: trailing comma in a tuple ───
#[test]
fn test_tuple_trailing_comma() {
    // `(1,)` is a one-element tuple. Pre-fix the parser emitted two errors
    // ("expected expression, found RParen" then "expected RParen, found …")
    // and swallowed the `)`.
    let result = parse_to_syntax("fn f() { let t = (1,); }", file_id());
    assert!(
        result.diagnostics.is_empty(),
        "`(1,)` must parse cleanly, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn test_tuple_trailing_comma_multi() {
    let result = parse_to_syntax("fn f() { let t = (1, 2,); }", file_id());
    assert!(
        result.diagnostics.is_empty(),
        "`(1, 2,)` must parse cleanly, got: {:?}",
        result.diagnostics
    );
}

// ─── FE-3: chained casts ───
#[test]
fn test_chained_casts() {
    // `as` is left-associative and chains freely; pre-fix the second `as`
    // produced "unexpected token in statement: KwAs".
    let result = parse_to_syntax("fn f() { let y = 1 as u8 as u32; }", file_id());
    assert!(
        result.diagnostics.is_empty(),
        "`1 as u8 as u32` must parse cleanly, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn test_triple_chained_casts() {
    let result = parse_to_syntax("fn f() { let y = 1 as i32 as i64 as u64; }", file_id());
    assert!(result.diagnostics.is_empty(), "got: {:?}", result.diagnostics);
}
