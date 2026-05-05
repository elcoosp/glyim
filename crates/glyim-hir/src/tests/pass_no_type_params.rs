use crate::passes::no_type_params::{assert_no_type_params, has_unresolved_param};
use crate::types::HirType;
use glyim_interner::Interner;

#[test]
fn has_unresolved_param_detects_generic() {
    #[allow(unused_mut)]
    let mut interner = Interner::new();
    let t = HirType::Named(interner.intern("T")); // single uppercase letter
    assert!(has_unresolved_param(&t, &interner));
}

#[test]
fn has_unresolved_param_ignores_concrete() {
    #[allow(unused_mut)]
    let mut interner = Interner::new();
    let t = HirType::Named(interner.intern("Vec"));
    assert!(!has_unresolved_param(&t, &interner));
}

#[test]
fn assert_no_type_params_panics_on_unresolved() {
    #[allow(unused_mut)]
    let mut interner = Interner::new();
    let expr = crate::node::HirExpr::SizeOf {
        id: crate::types::ExprId::new(0),
        target_type: HirType::Named(interner.intern("K")),
        span: glyim_diag::Span::new(0, 0),
    };
    let result = std::panic::catch_unwind(|| assert_no_type_params(&expr, &interner));
    assert!(result.is_err(), "Expected panic on unresolved type param");
}

#[test]
fn assert_no_type_params_ok_for_concrete() {
    #[allow(unused_mut)]
    let mut interner = Interner::new();
    let expr = crate::node::HirExpr::SizeOf {
        id: crate::types::ExprId::new(0),
        target_type: HirType::Int,
        span: glyim_diag::Span::new(0, 0),
    };
    let result = std::panic::catch_unwind(|| assert_no_type_params(&expr, &interner));
    assert!(result.is_ok(), "Expected no panic for concrete type");
}
