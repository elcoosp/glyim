use crate::{Hir, HirExpr, HirItem, HirStmt, HirType};
use glyim_interner::Interner;

/// Desugar all MethodCall expressions to Call expressions.
/// Uses concrete type information from the type checker (`expr_types`) to
/// mangle the callee name (e.g. `HashMap_insert__i64_i64`).
pub fn desugar_method_calls(hir: &mut Hir, expr_types: &[HirType], interner: &mut Interner) {
    for item in &mut hir.items {
        match item {
            HirItem::Fn(fn_def) => desugar_expr(&mut fn_def.body, expr_types, interner),
            HirItem::Impl(impl_def) => {
                for method in &mut impl_def.methods {
                    desugar_expr(&mut method.body, expr_types, interner);
                }
            }
            _ => {}
        }
    }
}

fn desugar_stmt(stmt: &mut HirStmt, expr_types: &[HirType], interner: &mut Interner) {
    match stmt {
        HirStmt::Let { value: e, .. }
        | HirStmt::LetPat { value: e, .. }
        | HirStmt::Assign { value: e, .. } => desugar_expr(e, expr_types, interner),
        HirStmt::AssignDeref { target, value, .. } => {
            desugar_expr(target, expr_types, interner);
            desugar_expr(value, expr_types, interner);
        }
        HirStmt::AssignField { object, value, .. } => {
            desugar_expr(object, expr_types, interner);
            desugar_expr(value, expr_types, interner);
        }
        HirStmt::Expr(e) => desugar_expr(e, expr_types, interner),
    }
}

fn concrete_type_name(ty: &HirType, interner: &Interner) -> String {
    crate::monomorphize::type_to_short_string(ty, interner)
}

fn desugar_expr(expr: &mut HirExpr, expr_types: &[HirType], interner: &mut Interner) {
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
                .unwrap_or(HirType::Int);
            let type_name = match &receiver_ty {
                HirType::Named(s) | HirType::Generic(s, _) => interner.resolve(*s).to_string(),
                _ => "unknown".to_string(),
            };
            let method = interner.resolve(*method_name).to_string();
            let base = format!("{}_{}", type_name, method);
            // Only append suffix if all type args are concrete (no unresolved type params).
            let mangled = match &receiver_ty {
                HirType::Generic(_, type_args) if !type_args.is_empty() => {
                    fn is_concrete(ty: &HirType, interner: &Interner) -> bool {
                        match ty {
                            HirType::Named(sym) => {
                                let s = interner.resolve(*sym);
                                // single uppercase letter → type parameter
                                if s.len() == 1 && s.chars().next().unwrap().is_uppercase() {
                                    false
                                } else {
                                    true
                                }
                            }
                            HirType::Generic(_, args) => {
                                args.iter().all(|a| is_concrete(a, interner))
                            }
                            HirType::Tuple(elems) => elems.iter().all(|e| is_concrete(e, interner)),
                            HirType::RawPtr(inner) => is_concrete(inner, interner),
                            HirType::Option(inner) => is_concrete(inner, interner),
                            HirType::Result(ok, err) => {
                                is_concrete(ok, interner) && is_concrete(err, interner)
                            }
                            _ => true,
                        }
                    }
                    let all_concrete = type_args.iter().all(|a| is_concrete(a, interner));
                    if all_concrete {
                        let suffix = type_args
                            .iter()
                            .map(|a| concrete_type_name(a, interner))
                            .collect::<Vec<_>>()
                            .join("_");
                        format!("{}__{}", base, suffix)
                    } else {
                        base
                    }
                }
                _ => base,
            };
            let callee = interner.intern(&mangled);
            tracing::debug!("[desugar] MethodCall {} → Call {}", method, mangled);

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
            desugar_expr(expr, expr_types, interner);
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                desugar_stmt(stmt, expr_types, interner);
            }
        }
        // ... rest same as before, passing expr_types and interner ...
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            desugar_expr(condition, expr_types, interner);
            desugar_expr(then_branch, expr_types, interner);
            if let Some(e) = else_branch {
                desugar_expr(e, expr_types, interner);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            desugar_expr(scrutinee, expr_types, interner);
            for arm in arms.iter_mut() {
                if let Some(ref mut g) = arm.guard {
                    desugar_expr(g, expr_types, interner);
                }
                desugar_expr(&mut arm.body, expr_types, interner);
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            desugar_expr(condition, expr_types, interner);
            desugar_expr(body, expr_types, interner);
        }
        HirExpr::ForIn { iter, body, .. } => {
            desugar_expr(iter, expr_types, interner);
            desugar_expr(body, expr_types, interner);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs, expr_types, interner);
            desugar_expr(rhs, expr_types, interner);
        }
        HirExpr::Unary { operand, .. } => desugar_expr(operand, expr_types, interner),
        HirExpr::Deref { expr: e, .. } => desugar_expr(e, expr_types, interner),
        HirExpr::FieldAccess { object, .. } => desugar_expr(object, expr_types, interner),
        HirExpr::As { expr: e, .. } => desugar_expr(e, expr_types, interner),
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                desugar_expr(val, expr_types, interner);
            }
        }
        HirExpr::EnumVariant { args, .. } => {
            for arg in args {
                desugar_expr(arg, expr_types, interner);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for elem in elements {
                desugar_expr(elem, expr_types, interner);
            }
        }
        HirExpr::Call { args, .. } => {
            for arg in args {
                desugar_expr(arg, expr_types, interner);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg, expr_types, interner),
        HirExpr::Assert {
            condition, message, ..
        } => {
            desugar_expr(condition, expr_types, interner);
            if let Some(msg) = message {
                desugar_expr(msg, expr_types, interner);
            }
        }
        // leaf nodes
        _ => {}
    }
}
