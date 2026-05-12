use crate::{Hir, HirExpr, HirItem, HirStmt, HirType};
use glyim_interner::Interner;

pub fn desugar_method_calls(hir: &mut Hir, expr_types: &[HirType], interner: &mut Interner) {
    for item in &mut hir.items {
        match item {
            HirItem::Fn(fn_def) => {
                let self_ty = fn_def.params.first().map(|(_, ty)| ty.clone());
                desugar_expr(&mut fn_def.body, expr_types, interner, self_ty.as_ref());
            }
            HirItem::Impl(impl_def) => {
                for method in &mut impl_def.methods {
                    let self_ty = method.params.first().map(|(_, ty)| ty.clone());
                    desugar_expr(&mut method.body, expr_types, interner, self_ty.as_ref());
                }
            }
            _ => {}
        }
    }
}

fn desugar_stmt(
    stmt: &mut HirStmt,
    expr_types: &[HirType],
    interner: &mut Interner,
    self_ty: Option<&HirType>,
) {
    match stmt {
        HirStmt::Let { value, .. }
        | HirStmt::LetPat { value, .. }
        | HirStmt::Assign { value, .. } => desugar_expr(value, expr_types, interner, self_ty),
        HirStmt::AssignDeref { target, value, .. } => {
            desugar_expr(target, expr_types, interner, self_ty);
            desugar_expr(value, expr_types, interner, self_ty);
        }
        HirStmt::AssignField { object, value, .. } => {
            desugar_expr(object, expr_types, interner, self_ty);
            desugar_expr(value, expr_types, interner, self_ty);
        }
        HirStmt::Expr(e) => desugar_expr(e, expr_types, interner, self_ty),
    }
}

fn resolve_type_name(ty: &HirType, interner: &Interner) -> Option<String> {
    match ty {
        HirType::Named(s) | HirType::Generic(s, _) => Some(interner.resolve(*s).to_string()),
        _ => None,
    }
}

fn desugar_expr(
    expr: &mut HirExpr,
    expr_types: &[HirType],
    interner: &mut Interner,
    self_ty: Option<&HirType>,
) {
    match expr {
        HirExpr::MethodCall {
            id,
            receiver,
            method_name,
            args,
            span,
            ..
        } => {
            let receiver_id = receiver.get_id();
            let receiver_ty = expr_types
                .get(receiver_id.as_usize())
                .cloned()
                .unwrap_or(HirType::Error);
            let method_str = interner.resolve(*method_name).to_string();
            eprintln!("[DESUGAR] receiver_ty={:?} method={}", receiver_ty, method_str);

            // Try to get type name from receiver, falling back to self_ty
            let type_name = if receiver_ty == HirType::Error || receiver_ty == HirType::Int {
                self_ty
                    .and_then(|ty| resolve_type_name(ty, interner))
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                resolve_type_name(&receiver_ty, interner).unwrap_or_else(|| "unknown".to_string())
            };

            let base = format!("{}_{}", type_name, method_str);
            let callee = interner.intern(&base);

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
            let mut full_args = vec![receiver_expr];
            full_args.append(&mut std::mem::take(args));
            *expr = HirExpr::Call {
                id,
                callee: Box::new(HirExpr::Ident {
                    id: crate::types::ExprId::new(0),
                    name: callee,
                    span: glyim_diag::Span::new(0, 0),
                }),
                args: full_args,
                span,
            };
            desugar_expr(expr, expr_types, interner, self_ty);
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                desugar_stmt(stmt, expr_types, interner, self_ty);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            desugar_expr(then_branch, expr_types, interner, self_ty);
            if let Some(e) = else_branch {
                desugar_expr(e, expr_types, interner, self_ty);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            desugar_expr(scrutinee, expr_types, interner, self_ty);
            for arm in arms.iter_mut() {
                if let Some(ref mut g) = arm.guard {
                    desugar_expr(g, expr_types, interner, self_ty);
                }
                desugar_expr(&mut arm.body, expr_types, interner, self_ty);
            }
        }
        HirExpr::While {
            condition, body, ..
        }
        | HirExpr::ForIn {
            iter: condition,
            body,
            ..
        } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            desugar_expr(body, expr_types, interner, self_ty);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs, expr_types, interner, self_ty);
            desugar_expr(rhs, expr_types, interner, self_ty);
        }
        HirExpr::Unary { operand, .. }
        | HirExpr::Deref { expr: operand, .. }
        | HirExpr::As { expr: operand, .. }
        | HirExpr::FieldAccess {
            object: operand, ..
        } => desugar_expr(operand, expr_types, interner, self_ty),
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                desugar_expr(v, expr_types, interner, self_ty);
            }
        }
        HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
            for a in args {
                desugar_expr(a, expr_types, interner, self_ty);
            }
        }
        HirExpr::Call { args, .. } => {
            for a in args {
                desugar_expr(a, expr_types, interner, self_ty);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg, expr_types, interner, self_ty),
        HirExpr::Assert {
            condition, message, ..
        } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            if let Some(m) = message {
                desugar_expr(m, expr_types, interner, self_ty);
            }
        }
        _ => {}
    }
}
