use crate::equivalence::are_equivalent;
use glyim_diag::Span;
use glyim_hir::node::{HirBinOp, HirExpr};
use glyim_hir::types::ExprId;
use glyim_interner::Interner;

#[test]
fn equivalent_x_plus_zero_equals_x() {
    let mut interner = Interner::new();
    let x_sym = interner.intern("x");
    let x = HirExpr::Ident { id: ExprId::new(1), name: x_sym, span: Span::new(0, 0) };
    let x_plus_0 = HirExpr::Binary {
        id: ExprId::new(2),
        op: HirBinOp::Add,
        lhs: Box::new(x.clone()),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(3), value: 0, span: Span::new(0, 0) }),
        span: Span::new(0, 0),
    };
    let types = vec![];
    let result = are_equivalent(&x_plus_0, &x, &types, &interner);
    assert!(result.equivalent, "x+0 should be equivalent to x");
}

#[test]
fn not_equivalent_x_plus_1_equals_x() {
    let mut interner = Interner::new();
    let x_sym = interner.intern("x");
    let x = HirExpr::Ident { id: ExprId::new(1), name: x_sym, span: Span::new(0, 0) };
    let x_plus_1 = HirExpr::Binary {
        id: ExprId::new(2),
        op: HirBinOp::Add,
        lhs: Box::new(x.clone()),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(3), value: 1, span: Span::new(0, 0) }),
        span: Span::new(0, 0),
    };
    let types = vec![];
    let result = are_equivalent(&x_plus_1, &x, &types, &interner);
    assert!(!result.equivalent, "x+1 should NOT be equivalent to x");
}
