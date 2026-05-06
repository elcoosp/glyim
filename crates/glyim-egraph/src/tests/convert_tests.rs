use crate::lang::GlyimExpr;
use crate::convert::{hir_expr_to_egraph, egraph_to_hir_expr};
use crate::analysis::GlyimAnalysis;
use egg::EGraph;
use glyim_diag::Span;
use glyim_hir::node::{HirExpr, HirBinOp};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use std::collections::HashMap;

#[test]
fn roundtrip_int_literal() {
    let expr = HirExpr::IntLit { id: ExprId::new(0), value: 42, span: Span::new(0, 0) };
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let mut interner = Interner::new();
    let mut type_map = HashMap::new();
    let id = hir_expr_to_egraph(&mut egraph, &expr, &mut interner, &[], &mut type_map);
    let mut next_id = 1;
    let result = egraph_to_hir_expr(&egraph, id, &mut interner, &mut next_id);
    assert!(matches!(result, HirExpr::IntLit { value: 42, .. }));
}

#[test]
fn roundtrip_binary_add() {
    let lhs = HirExpr::IntLit { id: ExprId::new(0), value: 1, span: Span::new(0, 0) };
    let rhs = HirExpr::IntLit { id: ExprId::new(1), value: 2, span: Span::new(0, 0) };
    let expr = HirExpr::Binary { id: ExprId::new(2), op: HirBinOp::Add, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Span::new(0, 0) };
    let mut egraph = EGraph::new(GlyimAnalysis::default());
    let mut interner = Interner::new();
    let mut type_map = HashMap::new();
    let id = hir_expr_to_egraph(&mut egraph, &expr, &mut interner, &[], &mut type_map);
    let mut next_id = 3;
    let result = egraph_to_hir_expr(&egraph, id, &mut interner, &mut next_id);
    assert!(matches!(result, HirExpr::Binary { op: HirBinOp::Add, .. }));
}
