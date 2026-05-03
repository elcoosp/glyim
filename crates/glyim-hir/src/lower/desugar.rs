use crate::{Hir, HirExpr, HirItem, HirStmt};
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Desugar all MethodCall expressions to Call expressions.
/// Relies on the type checker having populated `method_resolved` map.
pub fn desugar_method_calls(
    hir: &mut Hir,
    method_resolved: &HashMap<crate::types::ExprId, Symbol>,
) {
    for item in &mut hir.items {
        match item {
            HirItem::Fn(fn_def) => desugar_expr(&mut fn_def.body, method_resolved),
            HirItem::Impl(impl_def) => {
                for method in &mut impl_def.methods {
                    desugar_expr(&mut method.body, method_resolved);
                }
            }
            _ => {}
        }
    }
}

fn desugar_stmt(
    stmt: &mut HirStmt,
    map: &HashMap<crate::types::ExprId, Symbol>,
) {
    match stmt {
        HirStmt::Let { value: e, .. }
        | HirStmt::LetPat { value: e, .. }
        | HirStmt::Assign { value: e, .. } => desugar_expr(e, map),
        HirStmt::AssignDeref { target, value, .. } => {
            desugar_expr(target, map);
            desugar_expr(value, map);
        }
        HirStmt::AssignField {
            object, value, ..
        } => {
            desugar_expr(object, map);
            desugar_expr(value, map);
        }
        HirStmt::Expr(e) => desugar_expr(e, map),
    }
}

fn desugar_expr(
    expr: &mut HirExpr,
    map: &HashMap<crate::types::ExprId, Symbol>,
) {
    match expr {
        HirExpr::MethodCall {
            id,
            receiver,
            args,
            span,
            ..
        } => {
            if let Some(&callee) = map.get(id) {
                eprintln!("[desugar] MethodCall -> Call");
                let span = *span;
                let id = *id;
                let receiver_expr = *std::mem::replace(
                    receiver,
                    Box::new(HirExpr::IntLit {
                        id: crate::types::ExprId::new(0),
                        value: 0,
                        span: glyim_diag::Span::new(0, 0),
                    }),
                );
                let mut args_vec = std::mem::take(args);
                let mut full_args = vec![receiver_expr];
                full_args.append(&mut args_vec);
                *expr = HirExpr::Call {
                    id,
                    callee,
                    args: full_args,
                    span,
                };
                desugar_expr(expr, map);
            } else {
                desugar_expr(receiver, map);
                for arg in args {
                    desugar_expr(arg, map);
                }
            }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                desugar_stmt(stmt, map);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            desugar_expr(condition, map);
            desugar_expr(then_branch, map);
            if let Some(e) = else_branch {
                desugar_expr(e, map);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            desugar_expr(scrutinee, map);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    desugar_expr(g, map);
                }
                desugar_expr(body, map);
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            desugar_expr(condition, map);
            desugar_expr(body, map);
        }
        HirExpr::ForIn { iter, body, .. } => {
            desugar_expr(iter, map);
            desugar_expr(body, map);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs, map);
            desugar_expr(rhs, map);
        }
        HirExpr::Unary { operand, .. } => desugar_expr(operand, map),
        HirExpr::Deref { expr: e, .. } => desugar_expr(e, map),
        HirExpr::FieldAccess { object, .. } => desugar_expr(object, map),
        HirExpr::As { expr: e, .. } => desugar_expr(e, map),
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                desugar_expr(val, map);
            }
        }
        HirExpr::EnumVariant { args, .. } => {
            for arg in args {
                desugar_expr(arg, map);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for elem in elements {
                desugar_expr(elem, map);
            }
        }
        HirExpr::Call { args, .. } => {
            for arg in args {
                desugar_expr(arg, map);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg, map),
        HirExpr::Assert {
            condition, message, ..
        } => {
            desugar_expr(condition, map);
            if let Some(msg) = message {
                desugar_expr(msg, map);
            }
        }
        // leaf nodes
        HirExpr::IntLit { .. }
        | HirExpr::FloatLit { .. }
        | HirExpr::BoolLit { .. }
        | HirExpr::StrLit { .. }
        | HirExpr::Ident { .. }
        | HirExpr::UnitLit { .. }
        | HirExpr::SizeOf { .. }
        | HirExpr::AddrOf { .. }
        | HirExpr::Return { .. } => {}
    }
}
