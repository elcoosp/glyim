use glyim_hir::Hir;
use glyim_hir::monomorphize::monomorphize;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use std::collections::HashMap;

fn typecheck_source(source: &str) -> (Hir, Interner, Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
    let parse_out = glyim_parse::parse(source);
    assert!(parse_out.errors.is_empty());
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

    let mut typeck = crate::TypeChecker::new(interner.clone());
    typeck.check(&hir).expect("type check must succeed");
    let interner = typeck.interner.clone();
    let expr_types = typeck.expr_types;
    let call_type_args = typeck.call_type_args;

    (hir, interner, expr_types, call_type_args)
}

#[test]
fn enum_specialization_for_option_i64() {
    let source = r#"
enum Option<T> { Some(T), None }
main = () => {
    let x: Option<i64> = Option::Some(42);
    match x {
        Option::Some(v) => v,
        Option::None => 0,
    }
}
"#;
    let (hir, mut interner, expr_types, call_type_args) = typecheck_source(source);
    let result = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    let has_option_i64 = result.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Enum(e) = item {
            interner.resolve(e.name) == "Option__i64"
        } else {
            false
        }
    });
    assert!(has_option_i64, "Expected Option__i64 specialization");
}

#[test]
fn enum_specialization_for_nested_option() {
    let source = r#"
enum Option<T> { Some(T), None }
main = () => {
    let x: Option<Option<i64>> = Option::Some(Option::Some(42));
    match x {
        Option::Some(inner) => match inner {
            Option::Some(val) => val,
            Option::None => 0,
        },
        Option::None => 0,
    }
}
"#;
    let (hir, mut interner, expr_types, call_type_args) = typecheck_source(source);
    let result = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    let has_option_i64 = result.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Enum(e) = item {
            interner.resolve(e.name) == "Option__i64"
        } else {
            false
        }
    });
    let has_option_option_i64 = result.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Enum(e) = item {
            interner.resolve(e.name) == "Option__Option_i64"
        } else {
            false
        }
    });
    assert!(has_option_i64, "Expected Option__i64 specialization");
    assert!(
        has_option_option_i64,
        "Expected Option__Option_i64 specialization"
    );
}
