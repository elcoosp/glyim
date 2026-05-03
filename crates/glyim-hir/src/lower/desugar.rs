use crate::{Hir, HirExpr, HirItem, HirStmt};

/// Desugar all MethodCall expressions to Call expressions.
/// Must be called after type checking has populated `resolved_callee`.
pub fn desugar_method_calls(hir: &mut Hir) {
    for item in &mut hir.items {
        match item {
            HirItem::Fn(fn_def) => desugar_expr(&mut fn_def.body),
            HirItem::Impl(impl_def) => {
                for method in &mut impl_def.methods {
                    desugar_expr(&mut method.body);
                }
            }
            _ => {}
        }
    }
}

fn desugar_stmt(stmt: &mut HirStmt) {
    match stmt {
        HirStmt::Let { value, .. }
        | HirStmt::LetPat { value, .. }
        | HirStmt::Assign { value, .. } => desugar_expr(value),
        HirStmt::AssignDeref { target, value, .. } => {
            desugar_expr(target);
            desugar_expr(value);
        }
        HirStmt::AssignField { object, value, .. } => {
            desugar_expr(object);
            desugar_expr(value);
        }
        HirStmt::Expr(e) => desugar_expr(e),
    }
}

fn desugar_expr(expr: &mut HirExpr) {
    match expr {
        HirExpr::MethodCall {
            id,
            receiver,
            method_name: _,
            resolved_callee: Some(callee),
            args,
            span,
        } => {
            let span = *span;
            let id = *id;
            let callee = *callee;
            // Take ownership of the receiver and args
            let receiver_expr = *std::mem::replace(receiver, Box::new(HirExpr::IntLit {
                id: crate::types::ExprId::new(0),
                value: 0,
                span: glyim_diag::Span::new(0, 0),
            }));
            let mut args_vec = std::mem::take(args);
            // Build the call arguments: receiver + method args
            let mut full_args = vec![receiver_expr];
            full_args.append(&mut args_vec);
            *expr = HirExpr::Call {
                id,
                callee,
                args: full_args,
                span,
            };
            // The receiver and args are now gone from the original expression, so no double reference.
            // Recurse into the newly created Call expression
            desugar_expr(expr);
        }
        HirExpr::MethodCall { receiver, args, .. } => {
            // unresolved callee – leave as is (error will be reported later)
            desugar_expr(receiver);
            for arg in args {
                desugar_expr(arg);
            }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                desugar_stmt(stmt);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            desugar_expr(condition);
            desugar_expr(then_branch);
            if let Some(e) = else_branch {
                desugar_expr(e);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            desugar_expr(scrutinee);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    desugar_expr(g);
                }
                desugar_expr(body);
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            desugar_expr(condition);
            desugar_expr(body);
        }
        HirExpr::ForIn { iter, body, .. } => {
            desugar_expr(iter);
            desugar_expr(body);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs);
            desugar_expr(rhs);
        }
        HirExpr::Unary { operand, .. } => desugar_expr(operand),
        HirExpr::Deref { expr, .. } => desugar_expr(expr),
        HirExpr::FieldAccess { object, .. } => desugar_expr(object),
        HirExpr::As { expr, .. } => desugar_expr(expr),
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                desugar_expr(val);
            }
        }
        HirExpr::EnumVariant { args, .. } => {
            for arg in args {
                desugar_expr(arg);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for elem in elements {
                desugar_expr(elem);
            }
        }
        HirExpr::Call { args, .. } => {
            for arg in args {
                desugar_expr(arg);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg),
        HirExpr::Assert {
            condition, message, ..
        } => {
            desugar_expr(condition);
            if let Some(msg) = message {
                desugar_expr(msg);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{HirFn, HirExpr, HirBinOp};
    use crate::types::{ExprId, HirType};

    #[test]
    fn desugar_method_to_call() {
        let mut interner = glyim_interner::Interner::new();
        let callee_sym = interner.intern("my_func");
        let x_sym = interner.intern("x");
        let y_sym = interner.intern("y");

        let mut hir = Hir {
            items: vec![crate::item::HirItem::Fn(HirFn {
                doc: None,
                name: interner.intern("test"),
                type_params: vec![],
                params: vec![],
                param_mutability: vec![],
                ret: None,
                body: HirExpr::MethodCall {
                    id: ExprId::new(0),
                    receiver: Box::new(HirExpr::Ident {
                        id: ExprId::new(1),
                        name: x_sym,
                        span: glyim_diag::Span::new(0, 1),
                    }),
                    method_name: interner.intern("method"),
                    resolved_callee: Some(callee_sym),
                    args: vec![HirExpr::Ident {
                        id: ExprId::new(2),
                        name: y_sym,
                        span: glyim_diag::Span::new(0, 1),
                    }],
                    span: glyim_diag::Span::new(0, 2),
                },
                span: glyim_diag::Span::new(0, 3),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
            })],
        };

        desugar_method_calls(&mut hir);

        // Now the body should be a Call
        match &hir.items[0] {
            crate::item::HirItem::Fn(f) => match &f.body {
                HirExpr::Call { callee, args, .. } => {
                    assert_eq!(*callee, callee_sym);
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[0], HirExpr::Ident { name, .. } if *name == x_sym));
                    assert!(matches!(&args[1], HirExpr::Ident { name, .. } if *name == y_sym));
                }
                _ => panic!("Expected Call after desugaring"),
            },
            _ => panic!("Expected Fn item"),
        }
    }
}
