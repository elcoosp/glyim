use crate::analysis::GlyimAnalysis;
use crate::lang::{GlyimExpr, GlyimOp};
use egg::{EGraph, Id};
use glyim_diag::Span;
use glyim_hir::node::{HirExpr, HirStmt};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use std::collections::HashMap;

/// Convert a HirExpr into an e-graph, returning the root e-class Id.
pub fn hir_expr_to_egraph(
    egraph: &mut EGraph<GlyimExpr, GlyimAnalysis>,
    expr: &HirExpr,
    interner: &mut Interner,
    _types: &[HirType],
    type_map: &mut HashMap<Id, HirType>,
) -> Id {
    match expr {
        HirExpr::IntLit { value, .. } => {
            let id = egraph.add(GlyimExpr::num(*value));
            type_map.insert(id, HirType::Int);
            id
        }
        HirExpr::FloatLit { value, .. } => {
            let id = egraph.add(GlyimExpr::fnum(value.to_bits()));
            type_map.insert(id, HirType::Float);
            id
        }
        HirExpr::BoolLit { value, .. } => {
            let id = egraph.add(GlyimExpr::bool_lit(*value));
            type_map.insert(id, HirType::Bool);
            id
        }
        HirExpr::StrLit { value, .. } => {
            let id = egraph.add(GlyimExpr::str_lit(value));
            type_map.insert(id, HirType::Str);
            id
        }
        HirExpr::Ident { name, .. } => {
            let name_str = interner.resolve(*name).to_string();
            let id = egraph.add(GlyimExpr::var(&name_str));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let l = hir_expr_to_egraph(egraph, lhs, interner, _types, type_map);
            let r = hir_expr_to_egraph(egraph, rhs, interner, _types, type_map);
            let id = egraph.add(GlyimExpr::bin_op(*op, l, r));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); } else { type_map.insert(id, HirType::Int); }
            id
        }
        HirExpr::Unary { op, operand, .. } => {
            let inner = hir_expr_to_egraph(egraph, operand, interner, _types, type_map);
            let id = egraph.add(GlyimExpr::un_op(*op, inner));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::Call { callee, args, .. } => {
            let arg_ids: Vec<Id> = args.iter().map(|a| hir_expr_to_egraph(egraph, a, interner, _types, type_map)).collect();
            let callee_str = interner.resolve(*callee).to_string();
            let id = egraph.add(GlyimExpr::call(&callee_str, arg_ids));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            let cond = hir_expr_to_egraph(egraph, condition, interner, _types, type_map);
            let then = hir_expr_to_egraph(egraph, then_branch, interner, _types, type_map);
            let else_ = match else_branch { Some(e) => hir_expr_to_egraph(egraph, e, interner, _types, type_map), None => egraph.add(GlyimExpr::num(0)) };
            let id = egraph.add(GlyimExpr::if_expr(cond, then, else_));
            type_map.insert(id, HirType::Int);
            id
        }
        HirExpr::Block { stmts, .. } => {
            let mut last = None;
            for s in stmts {
                match s {
                    HirStmt::Expr(e) => { last = Some(hir_expr_to_egraph(egraph, e, interner, _types, type_map)); }
                    HirStmt::Let { value, .. } => { let vid = hir_expr_to_egraph(egraph, value, interner, _types, type_map); last = Some(vid); }
                    _ => {}
                }
            }
            last.unwrap_or_else(|| egraph.add(GlyimExpr::num(0)))
        }
        _ => egraph.add(GlyimExpr::num(0)),
    }
}

/// Extract a HirExpr from an e-class (simplified: first node).
pub fn egraph_to_hir_expr(
    egraph: &EGraph<GlyimExpr, GlyimAnalysis>,
    id: Id,
    interner: &mut Interner,
    next_expr_id: &mut u32,
) -> HirExpr {
    let node = &egraph[id].nodes[0];
    let id = ExprId::new(*next_expr_id);
    *next_expr_id += 1;
    match node.op {
        GlyimOp::Num => {
            let n: i64 = node.data.parse().unwrap_or(0);
            HirExpr::IntLit { id, value: n, span: Span::new(0, 0) }
        }
        GlyimOp::BoolLit => {
            let b: bool = node.data.parse().unwrap_or(false);
            HirExpr::BoolLit { id, value: b, span: Span::new(0, 0) }
        }
        GlyimOp::FNum => { HirExpr::IntLit { id, value: 0, span: Span::new(0, 0) } } // simplified
        GlyimOp::Var => {
            let sym = interner.intern(&node.data);
            HirExpr::Ident { id, name: sym, span: Span::new(0, 0) }
        }
        GlyimOp::BinOp(op) => {
            let lhs = egraph_to_hir_expr(egraph, node.children[0], interner, next_expr_id);
            let rhs = egraph_to_hir_expr(egraph, node.children[1], interner, next_expr_id);
            HirExpr::Binary { id, op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Span::new(0, 0) }
        }
        GlyimOp::UnOp(op) => {
            let operand = egraph_to_hir_expr(egraph, node.children[0], interner, next_expr_id);
            HirExpr::Unary { id, op, operand: Box::new(operand), span: Span::new(0, 0) }
        }
        GlyimOp::Call => {
            let callee = interner.intern(&node.data);
            let arg_exprs: Vec<HirExpr> = node.children.iter().map(|&c| egraph_to_hir_expr(egraph, c, interner, next_expr_id)).collect();
            HirExpr::Call { id, callee, args: arg_exprs, span: Span::new(0, 0) }
        }
        GlyimOp::If => {
            let cond = egraph_to_hir_expr(egraph, node.children[0], interner, next_expr_id);
            let then = egraph_to_hir_expr(egraph, node.children[1], interner, next_expr_id);
            let else_ = Box::new(egraph_to_hir_expr(egraph, node.children[2], interner, next_expr_id));
            HirExpr::If { id, condition: Box::new(cond), then_branch: Box::new(then), else_branch: Some(else_), span: Span::new(0, 0) }
        }
        _ => HirExpr::IntLit { id, value: 0, span: Span::new(0, 0) },
    }
}
