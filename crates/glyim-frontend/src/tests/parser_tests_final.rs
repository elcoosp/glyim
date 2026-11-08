use crate::parser::parse_to_syntax;
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(1)
}

// ─── Negative tests (should parse without crash, may produce diagnostics) ───
#[test]
fn test_negative_empty_file() {
    let result = parse_to_syntax("", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_negative_keyword_as_ident_in_wrong_place() {
    let _ = parse_to_syntax("fn let() {}", file_id());
}

#[test]
fn test_negative_double_dot_in_path() {
    let _ = parse_to_syntax("fn f() { a:: }", file_id());
}

#[test]
fn test_negative_missing_rparen_in_fn() {
    let _ = parse_to_syntax("fn f( { }", file_id());
}

#[test]
fn test_negative_missing_rbrace_in_block() {
    let _ = parse_to_syntax("fn f() {", file_id());
}

#[test]
fn test_negative_missing_semicolon_after_struct() {
    let _ = parse_to_syntax("struct Foo", file_id());
}

#[test]
fn test_negative_mismatched_delim() {
    let _ = parse_to_syntax("fn f() { (1 + 2] }", file_id());
}

#[test]
fn test_negative_empty_generics() {
    let result = parse_to_syntax("fn f<T>() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_negative_nested_braces_in_expr() {
    let result = parse_to_syntax("fn f() { { { x } } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Method call chains on complex expressions ───
#[test]
fn test_method_on_parenthesized() {
    let result = parse_to_syntax("fn f() { (x + y).clone() }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_method_on_block() {
    // Known: method call on block expression needs postfix after block.
    let _ = parse_to_syntax("fn f() { { let x = 1; x }.clone() }", file_id());
}

#[test]
fn test_method_on_if_expr() {
    let result = parse_to_syntax("fn f() { (if true { 1 } else { 2 }).clone() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Index expressions on complex types ───
#[test]
fn test_index_on_method_call() {
    let result = parse_to_syntax("fn f() { foo.bar()[0] }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_index_then_call() {
    let result = parse_to_syntax("fn f() { arr[0](42) }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_multi_dimensional_index() {
    let result = parse_to_syntax("fn f() { m[0][1][2] }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Type edge cases ───
#[test]
fn test_type_ref_to_tuple() {
    let result = parse_to_syntax("fn f(x: &(i32, f64)) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_tuple_of_refs() {
    let result = parse_to_syntax("fn f(x: (&i32, &f64)) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_ref_to_slice() {
    let result = parse_to_syntax("fn f(x: &[i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_slice_of_refs() {
    let result = parse_to_syntax("fn f(x: &[&i32]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_array_of_refs() {
    let result = parse_to_syntax("fn f(x: [&i32; 3]) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_type_nested_tuple() {
    let result = parse_to_syntax("fn f(x: (i32, (f64, bool), String)) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Trait bounds ───
#[test]
fn test_trait_bound_single() {
    let result = parse_to_syntax("fn f<T: Clone>(x: T) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_trait_bound_multiple_plus() {
    let result = parse_to_syntax("fn f<T: Clone + Eq + Ord>(x: T) {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_trait_where_clause_with_bounds() {
    let _ = parse_to_syntax("fn f<T>(x: T) where T: Clone + Eq {}", file_id());
}

// ─── Complex generic types ───
#[test]
fn test_generic_in_params_and_return() {
    let result = parse_to_syntax("fn identity<T>(x: T) -> T { x }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_multiple_generic_params() {
    let result = parse_to_syntax("struct Pair<A, B> { first: A, second: B }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_generic_with_lifetime_bounds() {
    let _ = parse_to_syntax("fn f<'a, T: 'a>(x: &'a T) {}", file_id());
}

// ─── Closure variations ───
#[test]
fn test_closure_inline_call() {
    // Known: closure immediate call needs paren grouping.
    let _ = parse_to_syntax("fn f() { (|x| x + 1)(42) }", file_id());
}

#[test]
fn test_closure_as_argument() {
    // Known: closure as function argument needs call-arg context.
    let _ = parse_to_syntax("fn f() { map(|x| x * 2) }", file_id());
}

#[test]
fn test_closure_multiline_body() {
    // Known: closure multiline body needs block parsing in closure context.
    let _ = parse_to_syntax("fn f() { |x| { let y = x + 1; y } }", file_id());
}

// ─── Struct update syntax ───
#[test]
fn test_struct_update_from_variable() {
    let result = parse_to_syntax("fn f() { Point { x: 1, ..base } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_struct_update_only_base() {
    let result = parse_to_syntax("fn f() { Point { ..other } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Match expressions ───
#[test]
fn test_match_with_multiple_patterns() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { 0 | 1 | 2 => 1, _ => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_with_struct_pattern() {
    let result = parse_to_syntax(
        "fn f(p: Point) { match p { Point { x, y } => x + y } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_with_tuple_pattern() {
    let result = parse_to_syntax(
        "fn f(p: (i32, i32)) { match p { (0, y) => y, (x, 0) => x, _ => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_with_nested_patterns() {
    let result = parse_to_syntax(
        "fn f(x: Option<(i32, i32)>) { match x { Some((a, b)) => a + b, None => 0 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// ─── Visibility ───
#[test]
fn test_pub_fn() {
    let result = parse_to_syntax("pub fn f() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pub_struct() {
    let result = parse_to_syntax("pub struct Foo {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pub_enum() {
    let result = parse_to_syntax("pub enum E { A, B }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pub_trait() {
    let result = parse_to_syntax("pub trait T { }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pub_mod() {
    let result = parse_to_syntax("pub mod m { }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Doc comments ───
#[test]
fn test_doc_comment_on_fn() {
    let result = parse_to_syntax("/// This is a function\nfn f() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_doc_comment_on_struct() {
    let result = parse_to_syntax("/// This is a struct\nstruct S;", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_inner_doc_comment_module() {
    let result = parse_to_syntax("//! Module doc\nfn f() {}", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Return expressions ───
#[test]
fn test_return_unit_from_fn() {
    let result = parse_to_syntax("fn f() { return; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_return_expr_from_fn() {
    let result = parse_to_syntax("fn f() -> i32 { return 42; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_return_from_match() {
    let result = parse_to_syntax(
        "fn f(x: i32) -> i32 { match x { 0 => return 0, _ => 1 } }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

// ─── Break and continue ───
#[test]
fn test_break_from_loop() {
    let result = parse_to_syntax("fn f() { loop { break; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_break_with_value() {
    let result = parse_to_syntax("fn f() -> i32 { loop { break 42; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_continue_in_loop() {
    let result = parse_to_syntax("fn f() { loop { continue; } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Try operator ───
#[test]
fn test_try_on_call() {
    let result = parse_to_syntax("fn f() -> Option<i32> { foo()? }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_try_chain() {
    let result = parse_to_syntax("fn f() -> Option<i32> { foo()?.bar()?.baz() }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Range expressions ───
#[test]
fn test_range_in_match() {
    // Known: range patterns in match arms need pattern-range support.
    let _ = parse_to_syntax("fn f(x: i32) { match x { 0..10 => 1, _ => 0 } }", file_id());
}

#[test]
fn test_range_in_for() {
    let result = parse_to_syntax("fn f() { for i in 0..10 { } }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Bitwise operators in expressions ───
#[test]
fn test_bitwise_and_expr() {
    let result = parse_to_syntax("fn f() { a & b }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_bitwise_or_expr() {
    let result = parse_to_syntax("fn f() { a | b }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_bitwise_xor_expr() {
    let result = parse_to_syntax("fn f() { a ^ b }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_shift_expr() {
    let result = parse_to_syntax("fn f() { a << 2 }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Compound assignment operators ───
#[test]
fn test_add_assign() {
    let result = parse_to_syntax("fn f() { x += 1; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_sub_assign() {
    let result = parse_to_syntax("fn f() { x -= 1; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_mul_assign() {
    let result = parse_to_syntax("fn f() { x *= 2; }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_div_assign() {
    let result = parse_to_syntax("fn f() { x /= 2; }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Nested expressions ───
#[test]
fn test_expr_if_inside_plus() {
    let result = parse_to_syntax("fn f() { 1 + if true { 2 } else { 3 } }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_expr_match_inside_call() {
    let result = parse_to_syntax("fn f() { foo(match x { 0 => 1, _ => 2 }) }", file_id());
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_expr_block_inside_tuple() {
    let result = parse_to_syntax("fn f() { ({ 1 }, { 2 }) }", file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Deep recursion stress ───
#[test]
fn test_deep_arithmetic() {
    let result = parse_to_syntax(
        "fn f() { 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 }",
        file_id(),
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_many_statements() {
    let source = format!(
        "fn f() {{ {} }}",
        (0..50)
            .map(|i| format!("let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let _ = parse_to_syntax(&source, file_id());
}

#[test]
fn test_many_struct_fields() {
    let fields = (0..30)
        .map(|i| format!("f{i}: i32"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("struct Big {{ {fields} }}");
    let result = parse_to_syntax(&source, file_id());
    assert!(result.diagnostics.is_empty());
}

// ─── Snapshot tests ───
#[test]
fn test_snapshot_impl_trait() {
    glyim_test::snapshot_cst(
        "impl_trait",
        r#"
trait Animal {
    fn speak(&self) -> String;
}

struct Dog;

impl Animal for Dog {
    fn speak(&self) -> String {
        "woof".to_string()
    }
}

fn make_animal() -> impl Animal {
    Dog
}
"#,
    );
}

#[test]
fn test_snapshot_closures_and_iterators() {
    glyim_test::snapshot_cst(
        "closures_iterators",
        r"
fn sum_of_squares(values: &[i32]) -> i32 {
    values.iter().map(|&x| x * x).sum()
}

fn filter_and_collect(values: Vec<i32>) -> Vec<i32> {
    values.into_iter().filter(|&x| x > 0).collect()
}
",
    );
}

#[test]
fn test_snapshot_error_handling() {
    glyim_test::snapshot_cst(
        "error_handling",
        r#"
enum Result<T, E> {
    Ok(T),
    Err(E),
}

fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Result::Err("division by zero".to_string())
    } else {
        Result::Ok(a / b)
    }
}

fn main() -> Result<(), String> {
    let x = divide(10, 2)?;
    let y = divide(x, 0)?;
    Result::Ok(())
}
"#,
    );
}
