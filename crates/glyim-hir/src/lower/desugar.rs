use crate::{Hir, HirExpr, HirItem, HirStmt, HirType};
use glyim_interner::Interner;

pub fn desugar_method_calls(hir: &mut Hir, expr_types: &[HirType], interner: &mut Interner) {
    for item in &mut hir.items {
        let fn_self_ty = match item {
            HirItem::Fn(f) => f.params.first().map(|(_, ty)| ty.clone()),
            HirItem::Impl(imp) => imp.methods.first().and_then(|m| m.params.first().map(|(_, ty)| ty.clone())),
            _ => None,
        };
        match item {
            HirItem::Fn(fn_def) => {
                let st = fn_def.params.first().map(|(_, ty)| ty.clone());
                desugar_expr(&mut fn_def.body, expr_types, interner, st.as_ref());
            }
            HirItem::Impl(impl_def) => {
                for method in &mut impl_def.methods {
                    let st = method.params.first().map(|(_, ty)| ty.clone());
                    desugar_expr(&mut method.body, expr_types, interner, st.as_ref());
                }
            }
            _ => {}
        }
    }
}

fn desugar_stmt(stmt: &mut HirStmt, expr_types: &[HirType], interner: &mut Interner, self_ty: Option<&HirType>) {
    match stmt {
        HirStmt::Let { value: e, .. }
        | HirStmt::LetPat { value: e, .. }
        | HirStmt::Assign { value: e, .. } => desugar_expr(e, expr_types, interner, self_ty),
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

fn desugar_expr(expr: &mut HirExpr, expr_types: &[HirType], interner: &mut Interner, self_ty: Option<&HirType>) {
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

            let effective_ty = if receiver_ty == HirType::Error {
                eprintln!("[DESUGAR] receiver_ty is Error, self_ty={:?}", self_ty);
                self_ty.cloned().unwrap_or(HirType::Error)
            } else {
                receiver_ty
            };
            eprintln!("[DESUGAR] effective_ty={:?}, method={}", effective_ty, interner.resolve(*method_name));

            let type_name_sym = match &effective_ty {
                HirType::Named(s) | HirType::Generic(s, _) => *s,
                _ => *method_name,
            };
            let type_args: Vec<HirType> = match &effective_ty {
                HirType::Generic(_, args) if !args.is_empty() => args.clone(),
                _ => vec![],
            };

            let mangled = crate::mangling::mangle_method_name(
                interner,
                type_name_sym,
                *method_name,
                &type_args,
            )
            .unwrap_or_else(|_| *method_name);
            let callee = mangled;

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
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            desugar_expr(then_branch, expr_types, interner, self_ty);
            if let Some(e) = else_branch {
                desugar_expr(e, expr_types, interner, self_ty);
            }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            desugar_expr(scrutinee, expr_types, interner, self_ty);
            for arm in arms.iter_mut() {
                if let Some(ref mut g) = arm.guard {
                    desugar_expr(g, expr_types, interner, self_ty);
                }
                desugar_expr(&mut arm.body, expr_types, interner, self_ty);
            }
        }
        HirExpr::While { condition, body, .. } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            desugar_expr(body, expr_types, interner, self_ty);
        }
        HirExpr::ForIn { iter, body, .. } => {
            desugar_expr(iter, expr_types, interner, self_ty);
            desugar_expr(body, expr_types, interner, self_ty);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs, expr_types, interner, self_ty);
            desugar_expr(rhs, expr_types, interner, self_ty);
        }
        HirExpr::Unary { operand, .. } => desugar_expr(operand, expr_types, interner, self_ty),
        HirExpr::Deref { expr: e, .. } => desugar_expr(e, expr_types, interner, self_ty),
        HirExpr::FieldAccess { object, .. } => desugar_expr(object, expr_types, interner, self_ty),
        HirExpr::As { expr: e, .. } => desugar_expr(e, expr_types, interner, self_ty),
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                desugar_expr(val, expr_types, interner, self_ty);
            }
        }
        HirExpr::EnumVariant { args, .. } => {
            for arg in args {
                desugar_expr(arg, expr_types, interner, self_ty);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for elem in elements {
                desugar_expr(elem, expr_types, interner, self_ty);
            }
        }
        HirExpr::Call { args, .. } => {
            for arg in args {
                desugar_expr(arg, expr_types, interner, self_ty);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg, expr_types, interner, self_ty),
        HirExpr::Assert { condition, message, .. } => {
            desugar_expr(condition, expr_types, interner, self_ty);
            if let Some(msg) = message {
                desugar_expr(msg, expr_types, interner, self_ty);
            }
        }
        _ => {}
    }
}
