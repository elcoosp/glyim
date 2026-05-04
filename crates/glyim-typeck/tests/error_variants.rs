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
#[case("fn main() { let mut x = 42; x }", &[])] // no errors expected
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
    &["UnknownField"]  // ExtraField not yet emitted; unknown field is reported instead
)]
// InvalidQuestion and IfConditionMustBeBool are not yet implemented in the type checker; tests deferred
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
