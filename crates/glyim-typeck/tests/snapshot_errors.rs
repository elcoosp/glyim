use glyim_hir::lower;
use glyim_parse::parse;
use glyim_typeck::TypeChecker;
use glyim_typeck::TypeError;

fn typecheck_errors(source: &str) -> Vec<TypeError> {
    let parse_out = parse(source);
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    tc.check(&hir).unwrap_err()
}

#[test]
fn snapshot_non_exhaustive_match_user_enum() {
    let src = "enum Color { Red, Green, Blue }\nfn main() -> i64 { match Color::Red { Color::Red => 1 } }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_invalid_cast_to_str() {
    let src = "main = () => 42 as Str";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_assign_to_immutable() {
    let src = "fn main() -> i64 { let x = 5; x = 10; x }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_deref_non_pointer() {
    let src = "fn main() -> i64 { let x = 42; *x }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_missing_field_in_struct_lit() {
    let src = "struct Point { x, y }\nfn main() -> i64 { Point { x: 1 } }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_unknown_field_access() {
    let src = "struct Point { x }\nfn main() -> i64 { let p = Point { x: 1 }; p.y }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}

#[test]
fn snapshot_return_type_mismatch() {
    let src = "fn foo() -> bool { 42 }\nfn main() -> i64 { foo() }";
    let errors = typecheck_errors(src);
    insta::assert_debug_snapshot!(errors);
}
