use glyim_typeck::TypeChecker;
use glyim_parse::parse;
use glyim_hir::lower;

fn typecheck_and_format(source: &str) -> (String, String) {
    let parse_out = parse(source);
    assert!(parse_out.errors.is_empty(), "parse errors: {:?}", parse_out.errors);
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner.clone());
    let _ = tc.check(&hir);

    let expr_types_fmt = format!("{:?}", tc.expr_types);
    let call_type_args_fmt = format!("{:?}", tc.call_type_args);
    (expr_types_fmt, call_type_args_fmt)
}

#[test]
fn snapshot_simple_main() {
    let (expr_types, call_type_args) = typecheck_and_format("main = () => 42");
    insta::assert_snapshot!("simple_main__expr_types", expr_types);
    insta::assert_snapshot!("simple_main__call_type_args", call_type_args);
}

#[test]
fn snapshot_generic_call() {
    let (expr_types, call_type_args) = typecheck_and_format(
        "fn id<T>(x: T) -> T { x }\nmain = () => id(42)"
    );
    insta::assert_snapshot!("generic_call__expr_types", expr_types);
    insta::assert_snapshot!("generic_call__call_type_args", call_type_args);
}

#[test]
fn snapshot_generic_struct() {
    let (expr_types, call_type_args) = typecheck_and_format(
        "struct Container<T> { value: T }\nmain = () => { let c: Container<i64> = Container { value: 42 }; c.value }"
    );
    insta::assert_snapshot!("generic_struct__expr_types", expr_types);
    insta::assert_snapshot!("generic_struct__call_type_args", call_type_args);
}
