use glyim_hir::lower;
use glyim_parse::parse;
use glyim_typeck::TypeChecker;
use glyim_typeck::TypeError;
use rstest::rstest;

fn typecheck_source(source: &str) -> Vec<TypeError> {
    let parse_out = parse(source);
    assert!(
        parse_out.errors.is_empty(),
        "parse errors: {:?}",
        parse_out.errors
    );
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    let _ = tc.check(&hir);
    tc.errors
}

#[rstest]
#[case("fn main() { let mut x = 42; x }", &[])]
#[case("fn main() { let x = 5; x = 10; x }", &["AssignToImmutable"])]
#[case("fn main() { let x = 42; *x }", &["DerefNonPointer"])]
#[case("fn main() { let x = 42; *x = 10; }", &["AssignThroughNonPointer"])]
#[case(
    "enum Color { Red, Green }\nfn main() -> i64 { match Color::Red { Color::Red => 1 } }",
    &["NonExhaustiveMatch"]
)]
#[case(
    "fn foo() -> bool { 42 }\nfn main() -> i64 { foo() }",
    &["InvalidReturnType"]
)]
#[case(
    "fn take_bool(b: bool) -> i64 { 0 }\nfn main() -> i64 { take_bool(42) }",
    &["MismatchedTypes"]
)]
#[case(
    "struct Point { x }\nfn main() -> i64 { let p = Point { x: 1 }; p.y }",
    &["UnknownField"]
)]
#[case(
    "struct Point { x, y }\nfn main() -> i64 { Point { x: 1 } }",
    &["MissingField"]
)]
#[case(
    "struct Point { x }\nfn main() -> i64 { Point { x: 1, y: 2 } }",
    &["UnknownField"]
)]
fn detects_error_variant(#[case] source: &str, #[case] expected_patterns: &[&str]) {
    let errors = typecheck_source(source);

    if expected_patterns.is_empty() {
        assert!(errors.is_empty(), "expected no errors but got {:?}", errors);
        return;
    }

    for expected in expected_patterns {
        let found = errors.iter().any(|e| format!("{:?}", e).contains(expected));
        assert!(
            found,
            "Expected error containing '{}' but got {:?}\nSource:\n{}",
            expected, errors, source
        );
    }
}

#[test]
fn non_exhaustive_user_enum() {
    let src = "enum Color { Red, Green, Blue }\nfn main() -> i64 { match Color::Red { Color::Red => 1 } }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::NonExhaustiveMatch { .. })),
        "expected NonExhaustiveMatch error, got {:?}",
        errors
    );
}

#[test]
fn invalid_cast_to_str() {
    let src = "main = () => 42 as Str";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::MismatchedTypes { .. })),
        "expected MismatchedTypes error for int->Str cast, got {:?}",
        errors
    );
}

#[test]
fn assign_to_immutable_error() {
    let src = "fn main() -> i64 { let x = 5; x = 10; x }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::AssignToImmutable { .. })),
        "expected AssignToImmutable error, got {:?}",
        errors
    );
}

#[test]
fn deref_non_pointer_error() {
    let src = "fn main() -> i64 { let x = 42; *x }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::DerefNonPointer { .. })),
        "expected DerefNonPointer error, got {:?}",
        errors
    );
}

#[test]
fn assign_through_non_pointer_error() {
    let src = "fn main() -> i64 { let x = 5; *x = 10; x }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::AssignThroughNonPointer { .. })),
        "expected AssignThroughNonPointer error, got {:?}",
        errors
    );
}

#[test]
fn method_call_on_primitive_no_panic() {
    let src = "fn main() -> i64 { let x = 42; x.push(); x }";
    let errors = typecheck_source(src);
    // Currently no error is emitted for unknown methods, just ensure no crash
    eprintln!("method_call_on_primitive errors: {:?}", errors);
}

#[test]
fn unknown_field_access_error() {
    let src = "struct Point { x }\nfn main() -> i64 { let p = Point { x: 1 }; p.y }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::UnknownField { .. })),
        "expected UnknownField error, got {:?}",
        errors
    );
}

#[test]
fn missing_field_in_struct_lit_error() {
    let src = "struct Point { x, y }\nfn main() -> i64 { Point { x: 1 } }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::MissingField { .. })),
        "expected MissingField error, got {:?}",
        errors
    );
}

#[test]
fn extra_field_in_struct_lit_error() {
    let src = "struct Point { x }\nfn main() -> i64 { Point { x: 1, y: 2 } }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::UnknownField { .. })),
        "expected UnknownField error for extra field, got {:?}",
        errors
    );
}

#[test]
fn return_type_mismatch_error() {
    let src = "fn foo() -> bool { 42 }\nfn main() -> i64 { foo() }";
    let errors = typecheck_source(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, glyim_typeck::TypeError::InvalidReturnType { .. })),
        "expected InvalidReturnType error, got {:?}",
        errors
    );
}
