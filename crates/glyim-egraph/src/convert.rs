use crate::analysis::GlyimAnalysis;
use crate::lang::GlyimLang;
use egg::{EGraph, Id};
use glyim_diag::Span;
use glyim_hir::node::{HirBinOp, HirExpr, HirStmt, HirUnOp};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use std::collections::HashMap;

pub fn hir_expr_to_egraph(
    egraph: &mut EGraph<GlyimLang, GlyimAnalysis>,
    expr: &HirExpr,
    interner: &Interner,
    _types: &[HirType],
    type_map: &mut HashMap<Id, HirType>,
) -> Id {
    match expr {
        HirExpr::IntLit { value, .. } => { let id = egraph.add(GlyimLang::Num(*value)); type_map.insert(id, HirType::Int); id }
        HirExpr::FloatLit { value, .. } => { let id = egraph.add(GlyimLang::FNum(value.to_bits())); type_map.insert(id, HirType::Float); id }
        HirExpr::BoolLit { value, .. } => { let id = egraph.add(GlyimLang::BoolLit(*value)); type_map.insert(id, HirType::Bool); id }
        HirExpr::StrLit { value, .. } => { let id = egraph.add(GlyimLang::StrLit(value.clone())); type_map.insert(id, HirType::Str); id }
        HirExpr::Ident { name, .. } => {
            let name_str = interner.resolve(*name).to_string();
            let id = egraph.add(GlyimLang::Var(name_str));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::Binary { op, lhs, rhs, .. } => {
            let l = hir_expr_to_egraph(egraph, lhs, interner, _types, type_map);
            let r = hir_expr_to_egraph(egraph, rhs, interner, _types, type_map);
            let lang = match op {
                HirBinOp::Add => GlyimLang::Add([l, r]),
                HirBinOp::Sub => GlyimLang::Sub([l, r]),
                HirBinOp::Mul => GlyimLang::Mul([l, r]),
                HirBinOp::Div => GlyimLang::Div([l, r]),
                HirBinOp::Mod => GlyimLang::Rem([l, r]),
                HirBinOp::Eq => GlyimLang::Eq([l, r]),
                HirBinOp::Neq => GlyimLang::Neq([l, r]),
                HirBinOp::Lt => GlyimLang::Lt([l, r]),
                HirBinOp::Gt => GlyimLang::Gt([l, r]),
                HirBinOp::Lte => GlyimLang::Lte([l, r]),
                HirBinOp::Gte => GlyimLang::Gte([l, r]),
                HirBinOp::And => GlyimLang::And([l, r]),
                HirBinOp::Or => GlyimLang::Or([l, r]),
            };
            let id = egraph.add(lang);
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); } else { type_map.insert(id, HirType::Int); }
            id
        }
        HirExpr::Unary { op, operand, .. } => {
            let inner = hir_expr_to_egraph(egraph, operand, interner, _types, type_map);
            let lang = match op {
                HirUnOp::Neg => GlyimLang::Neg(inner),
                HirUnOp::Not => GlyimLang::Not(inner),
            };
            let id = egraph.add(lang);
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::Call { callee, args, .. } => {
            let arg_ids: Vec<Id> = args.iter().map(|a| hir_expr_to_egraph(egraph, a, interner, _types, type_map)).collect();
            let callee_str = interner.resolve(*callee).to_string();
            let id = egraph.add(GlyimLang::Call(callee_str, arg_ids));
            if let Some(ty) = _types.get(expr.get_id().as_usize()) { type_map.insert(id, ty.clone()); }
            id
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            let cond = hir_expr_to_egraph(egraph, condition, interner, _types, type_map);
            let then = hir_expr_to_egraph(egraph, then_branch, interner, _types, type_map);
            let else_ = match else_branch { Some(e) => hir_expr_to_egraph(egraph, e, interner, _types, type_map), None => egraph.add(GlyimLang::Num(0)) };
            let id = egraph.add(GlyimLang::If([cond, then, else_]));
            type_map.insert(id, HirType::Int);
            id
        }
        HirExpr::While { condition, body, .. } => {
            let cond = hir_expr_to_egraph(egraph, condition, interner, _types, type_map);
            let body_id = hir_expr_to_egraph(egraph, body, interner, _types, type_map);
            let id = egraph.add(GlyimLang::While([cond, body_id]));
            type_map.insert(id, HirType::Unit);
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
            last.unwrap_or_else(|| egraph.add(GlyimLang::Num(0)))
        }
        _ => egraph.add(GlyimLang::Num(0)),
    }
}

pub fn egraph_to_hir_expr(
    egraph: &EGraph<GlyimLang, GlyimAnalysis>,
    id: Id,
    interner: &mut Interner,
    next_expr_id: &mut u32,
) -> HirExpr {
    let node = &egraph[id].nodes[0];
    let eid = ExprId::new(*next_expr_id);
    *next_expr_id += 1;
    match node {
        GlyimLang::Num(n) => HirExpr::IntLit { id: eid, value: *n, span: Span::new(0, 0) },
        GlyimLang::BoolLit(b) => HirExpr::BoolLit { id: eid, value: *b, span: Span::new(0, 0) },
        GlyimLang::FNum(bits) => HirExpr::FloatLit { id: eid, value: f64::from_bits(*bits), span: Span::new(0, 0) },
        GlyimLang::Var(s) => {
            let sym = interner.intern(s);
            HirExpr::Ident { id: eid, name: sym, span: Span::new(0, 0) }
        }
        GlyimLang::Add([l, r]) => bin_hir(eid, HirBinOp::Add, l, r, egraph, interner, next_expr_id),
        GlyimLang::Sub([l, r]) => bin_hir(eid, HirBinOp::Sub, l, r, egraph, interner, next_expr_id),
        GlyimLang::Mul([l, r]) => bin_hir(eid, HirBinOp::Mul, l, r, egraph, interner, next_expr_id),
        GlyimLang::Div([l, r]) => bin_hir(eid, HirBinOp::Div, l, r, egraph, interner, next_expr_id),
        GlyimLang::Rem([l, r]) => bin_hir(eid, HirBinOp::Mod, l, r, egraph, interner, next_expr_id),
        GlyimLang::Eq([l, r]) => bin_hir(eid, HirBinOp::Eq, l, r, egraph, interner, next_expr_id),
        GlyimLang::Neq([l, r]) => bin_hir(eid, HirBinOp::Neq, l, r, egraph, interner, next_expr_id),
        GlyimLang::Lt([l, r]) => bin_hir(eid, HirBinOp::Lt, l, r, egraph, interner, next_expr_id),
        GlyimLang::Gt([l, r]) => bin_hir(eid, HirBinOp::Gt, l, r, egraph, interner, next_expr_id),
        GlyimLang::Lte([l, r]) => bin_hir(eid, HirBinOp::Lte, l, r, egraph, interner, next_expr_id),
        GlyimLang::Gte([l, r]) => bin_hir(eid, HirBinOp::Gte, l, r, egraph, interner, next_expr_id),
        GlyimLang::And([l, r]) => bin_hir(eid, HirBinOp::And, l, r, egraph, interner, next_expr_id),
        GlyimLang::Or([l, r]) => bin_hir(eid, HirBinOp::Or, l, r, egraph, interner, next_expr_id),
        GlyimLang::Neg(inner) => {
            HirExpr::Unary { id: eid, op: HirUnOp::Neg, operand: Box::new(egraph_to_hir_expr(egraph, *inner, interner, next_expr_id)), span: Span::new(0, 0) }
        }
        GlyimLang::Not(inner) => {
            HirExpr::Unary { id: eid, op: HirUnOp::Not, operand: Box::new(egraph_to_hir_expr(egraph, *inner, interner, next_expr_id)), span: Span::new(0, 0) }
        }
        GlyimLang::Call(name, args) => {
            let callee = interner.intern(name);
            let arg_exprs: Vec<HirExpr> = args.iter().map(|c| egraph_to_hir_expr(egraph, *c, interner, next_expr_id)).collect();
            HirExpr::Call { id: eid, callee, args: arg_exprs, span: Span::new(0, 0) }
        }
        GlyimLang::If([cond, then, else_]) => {
            HirExpr::If {
                id: eid,
                condition: Box::new(egraph_to_hir_expr(egraph, *cond, interner, next_expr_id)),
                then_branch: Box::new(egraph_to_hir_expr(egraph, *then, interner, next_expr_id)),
                else_branch: Some(Box::new(egraph_to_hir_expr(egraph, *else_, interner, next_expr_id))),
                span: Span::new(0, 0),
            }
        }
        _ => HirExpr::IntLit { id: eid, value: 0, span: Span::new(0, 0) },
    }
}

fn bin_hir(eid: ExprId, op: HirBinOp, l: &Id, r: &Id, egraph: &EGraph<GlyimLang, GlyimAnalysis>, interner: &mut Interner, next: &mut u32) -> HirExpr {
    HirExpr::Binary {
        id: eid, op,
        lhs: Box::new(egraph_to_hir_expr(egraph, *l, interner, next)),
        rhs: Box::new(egraph_to_hir_expr(egraph, *r, interner, next)),
        span: Span::new(0, 0),
    }
}
