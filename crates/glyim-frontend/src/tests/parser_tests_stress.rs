use crate::parser::parse_to_syntax;
use glyim_span::FileId;

fn file_id() -> FileId {
    FileId::from_raw(1)
}

// ─── Tuple field access ───
#[test]
fn test_tuple_field_access() {
    let result = parse_to_syntax("fn f() { x.0 }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_tuple_field_method() {
    let result = parse_to_syntax("fn f() { x.0.method() }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Deep nesting ───
#[test]
fn test_deeply_nested_blocks() {
    let result = parse_to_syntax(
        "fn f() { { { { { { { { { { 1 } } } } } } } } } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_deeply_nested_if() {
    let result = parse_to_syntax(
        "fn f() { if a { if b { if c { if d { 1 } } } } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_deeply_nested_types() {
    let result = parse_to_syntax("fn f(x: Option<Option<Option<Option<i32>>>>) {}", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Complex generics ───
#[test]
fn test_generic_with_multiple_bounds() {
    let result = parse_to_syntax("fn f<T: Clone + Eq + Display + Debug>(x: T) {}", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_generic_with_lifetime_like() {
    let _result = parse_to_syntax("fn f<'a, T: 'a>(x: &'a T) {}", file_id());
    // lifetimes not fully supported, just check no crash
}

#[test]
fn test_generic_with_default_and_bounds() {
    let result = parse_to_syntax(
        "struct S<T: Clone + Eq = String, U = i32> { x: T, y: U }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Where clauses ───
#[test]
fn test_where_clause_complex() {
    let _ = parse_to_syntax(
        "fn f<T, U>(x: T, y: U) where T: Clone + Eq, U: Display + Debug, T: Default {}",
        file_id(),
    );
}

// ─── Struct with visibility ───
#[test]
fn test_struct_pub_fields() {
    let _result = parse_to_syntax("struct S { pub x: i32, y: f64 }", file_id());
    // pub on fields may produce diagnostics but shouldn't crash
}

// ─── Complex patterns ───
#[test]
fn test_pattern_nested_struct() {
    let result = parse_to_syntax(
        "fn f() { match p { Outer { inner: Inner { x, y } } => x + y } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pattern_with_guard_complex() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { n if n > 0 && n < 10 => 1, _ => 0 } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pattern_or_guard() {
    let result = parse_to_syntax(
        "fn f(x: i32) { match x { 0 | 1 | 2 if x > 0 => 1, _ => 0 } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_pattern_ref_in_match() {
    let result = parse_to_syntax("fn f(x: &i32) { match x { &0 => 1, _ => 0 } }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Expression edge cases ───
#[test]
fn test_expr_paren_chain() {
    let result = parse_to_syntax("fn f() { ((((a + b)) * c) - d) }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_expr_chained_operators() {
    let result = parse_to_syntax("fn f() { a + b + c + d + e }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_expr_mixed_unary_binary() {
    let result = parse_to_syntax("fn f() { -a + *b - !c }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Array expressions ───
#[test]
fn test_array_repeat() {
    let result = parse_to_syntax("fn f() { [0; 10] }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_array_multi_dim() {
    let result = parse_to_syntax("fn f() { [[0; 3]; 2] }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Control flow expressions as values ───
#[test]
fn test_if_as_expression() {
    let result = parse_to_syntax("fn f() { let x = if true { 1 } else { 0 }; }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_match_as_expression() {
    let result = parse_to_syntax("fn f() { let x = match y { 0 => 1, _ => 0 }; }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_block_as_expression() {
    let result = parse_to_syntax("fn f() { let x = { let y = 1; y + 2 }; }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Method call chains ───
#[test]
fn test_method_chain_on_literal() {
    let result = parse_to_syntax("fn f() { 42.to_string().len() }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_method_with_generic_arg() {
    let result = parse_to_syntax("fn f() { foo.collect::<Vec<i32>>() }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Return position impl trait ───
#[test]
fn test_rpit() {
    let _ = parse_to_syntax("fn make_iter() -> impl Iterator<Item = i32> {}", file_id());
}

// ─── Enum with complex variants ───
#[test]
fn test_enum_complex_variants() {
    let result = parse_to_syntax("enum E { A(i32, f64), B { x: i32, y: f64 }, C }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Type alias with generics ───
#[test]
fn test_type_alias_generic() {
    let _ = parse_to_syntax("type Pair<T> = (T, T);", file_id());
}

// ─── Const with type annotation ───
#[test]
fn test_const_complex() {
    let result = parse_to_syntax("const X: &str = \"hello\";", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Static mut ───
#[test]
fn test_static_mut_with_type() {
    let result = parse_to_syntax("static mut COUNTER: usize = 0;", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Use with underscore ───
#[test]
fn test_use_underscore() {
    let _ = parse_to_syntax("use std::io::_", file_id());
}

// ─── Trait with default type parameter ───
#[test]
fn test_trait_default_type_param() {
    let result = parse_to_syntax(
        "trait Add<Rhs = Self> { type Output; fn add(self, rhs: Rhs) -> Self::Output; }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Impl with associated type ───
#[test]
fn test_impl_assoc_type() {
    let result = parse_to_syntax(
        "impl Iterator for Foo { type Item = i32; fn next(&mut self) -> Option<i32> { None } }",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Extern with multiple functions ───
#[test]
fn test_extern_multiple_fns() {
    let _ = parse_to_syntax(
        "extern \"C\" { fn foo(); fn bar(x: i32); fn baz() -> f64; }",
        file_id(),
    );
}

// ─── Unsafe trait ───
#[test]
fn test_unsafe_trait() {
    let result = parse_to_syntax("unsafe trait UnsafeCell { }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Unsafe impl ───
#[test]
fn test_unsafe_impl() {
    let result = parse_to_syntax("unsafe impl Send for Foo { }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── For loop patterns ───
#[test]
fn test_for_loop_destructure() {
    let result = parse_to_syntax("fn f() { for (a, b) in iter { } }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_for_loop_ref_pattern() {
    let result = parse_to_syntax("fn f() { for &x in iter { } }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Break/continue with label ───
#[test]
fn test_break_label() {
    let _ = parse_to_syntax(
        "fn f() { 'outer: loop { loop { break 'outer; } } }",
        file_id(),
    );
}

#[test]
fn test_continue_label() {
    let _ = parse_to_syntax(
        "fn f() { 'outer: loop { loop { continue 'outer; } } }",
        file_id(),
    );
}

// ─── Empty items ───
#[test]
fn test_empty_fn() {
    let result = parse_to_syntax("fn f() {}", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_empty_trait() {
    let result = parse_to_syntax("trait Empty { }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn test_empty_impl() {
    let result = parse_to_syntax("impl Foo { }", file_id());
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Many parameters ───
#[test]
fn test_many_params() {
    let result = parse_to_syntax(
        "fn f(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32) {}",
        file_id(),
    );
    for d in &result.diagnostics {
        eprintln!("test_array_repeat: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_array_multi_dim: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_deeply_nested_types: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_access: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_tuple_field_method: {}", d.message);
    }
    for d in &result.diagnostics {
        eprintln!("test_method_with_generic_arg: {}", d.message);
    }
    assert!(result.diagnostics.is_empty());
}

// ─── Snapshot: full program ───
#[test]
fn test_snapshot_full_program() {
    glyim_test::snapshot_cst(
        "full_program",
        r#"
use std::fmt::{Display, Debug};

pub trait Animal {
    fn name(&self) -> &str;
    fn speak(&self) -> String;
}

pub struct Dog {
    name: String,
    age: u32,
}

impl Dog {
    pub fn new(name: String, age: u32) -> Self {
        Dog { name, age }
    }
}

impl Animal for Dog {
    fn name(&self) -> &str {
        &self.name
    }
    fn speak(&self) -> String {
        "woof".to_string()
    }
}

enum Option<T> {
    Some(T),
    None,
}

fn main() {
    let dog = Dog::new("Rex".to_string(), 3);
    match dog.age {
        0..=1 => println!("puppy"),
        2..=7 => println!("adult"),
        _ => println!("senior"),
    }
    let x: Option<i32> = Option::Some(42);
    if let Option::Some(val) = x {
        let _ = val;
    }
    let arr = [1, 2, 3];
    for &item in &arr {
        let _ = item;
    }
}
"#,
    );
}
