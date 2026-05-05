use glyim_hir::lower;
use glyim_parse::parse;
use crate::TypeChecker;
use proptest::prelude::*;

fn typecheck_source(source: &str) -> Vec<crate::TypeError> {
    let parse_out = parse(source);
    if !parse_out.errors.is_empty() {
        return vec![]; // skip parse errors
    }
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    let _ = tc.check(&hir);
    tc.errors
}

/// Strategy that generates a valid Glyim integer expression.
fn arb_int_expr() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        5 => any::<i64>().prop_map(|n| n.to_string()),
        1 => Just("(__size_of::<i64>())".to_string()),
        1 => Just("(42)".to_string()),
    ];
    leaf.prop_recursive(4, 16, 4, |inner| {
        prop_oneof![
            2 => (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} + {r})")),
            2 => (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} - {r})")),
            1 => (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} * {r})")),
            1 => (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} / {r})")),
        ]
    })
}

proptest! {
    #[test]
    fn random_int_expressions_typecheck(expr in arb_int_expr()) {
        let source = format!("main = () => {}", expr);
        let errors = typecheck_source(&source);
        prop_assert!(errors.is_empty(), "unexpected type errors: {:?}", errors);
    }
}

/// Strategy that generates a valid block with let bindings and arithmetic.
fn arb_block() -> impl Strategy<Value = String> {
    let int = any::<i64>().prop_map(|n| n.to_string());
    (int.clone(), int.clone(), int.clone())
        .prop_map(|(x, y, z)| format!("let a = {x}\nlet b = {y}\nlet c = {z}\na + b * c"))
}

proptest! {
    #[test]
    fn random_blocks_typecheck(block in arb_block()) {
        let source = format!("main = () => {{ {} }}", block);
        let errors = typecheck_source(&source);
        prop_assert!(errors.is_empty(), "unexpected type errors: {:?}", errors);
    }
}
