use crate::{Hir, HirExpr, HirItem, HirStmt};
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Desugar all MethodCall expressions to Call expressions.
/// Must be called after type checking has populated the `method_resolved` map.
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

fn desugar_stmt(stmt: &mut HirStmt, method_resolved: &HashMap<crate::types::ExprId, Symbol>) {
    match stmt {
        HirStmt::Let { value: e, .. }
        | HirStmt::LetPat { value: e, .. }
        | HirStmt::Assign { value: e, .. } => desugar_expr(e, method_resolved),
        HirStmt::AssignDeref { target, value, .. } => {
            desugar_expr(target, method_resolved);
            desugar_expr(value, method_resolved);
        }
        HirStmt::AssignField { object, value, .. } => {
            desugar_expr(object, method_resolved);
            desugar_expr(value, method_resolved);
        }
        HirStmt::Expr(e) => desugar_expr(e, method_resolved),
    }
}

fn desugar_expr(expr: &mut HirExpr, method_resolved: &HashMap<crate::types::ExprId, Symbol>) {
    match expr {
        HirExpr::MethodCall {
            id,
            receiver,
            args,
            span,
            ..
        } => {
            if let Some(&callee) = method_resolved.get(id) {
                // Take ownership of receiver and args without cloning
                let receiver_expr = *std::mem::replace(
                    receiver,
                    Box::new(HirExpr::IntLit {
                        id: crate::types::ExprId::new(0),
                        value: 0,
                        span: glyim_diag::Span::new(0, 0),
                    }),
                );
                let mut args_vec = std::mem::take(args);
                let mut full_args: Vec<HirExpr> = vec![receiver_expr];
                full_args.append(&mut args_vec);
                *expr = HirExpr::Call {
                    id: *id,
                    callee,
                    args: full_args,
                    span: *span,
                };
                // Recurse on the new Call (its arguments are already desugared, but just in case)
                desugar_expr(expr, method_resolved);
            } else {
                // No resolved callee – still recurse into children
                desugar_expr(receiver, method_resolved);
                for arg in args {
                    desugar_expr(arg, method_resolved);
                }
            }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                desugar_stmt(stmt, method_resolved);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            desugar_expr(condition, method_resolved);
            desugar_expr(then_branch, method_resolved);
            if let Some(e) = else_branch {
                desugar_expr(e, method_resolved);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            desugar_expr(scrutinee, method_resolved);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    desugar_expr(g, method_resolved);
                }
                desugar_expr(body, method_resolved);
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            desugar_expr(condition, method_resolved);
            desugar_expr(body, method_resolved);
        }
        HirExpr::ForIn { iter, body, .. } => {
            desugar_expr(iter, method_resolved);
            desugar_expr(body, method_resolved);
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            desugar_expr(lhs, method_resolved);
            desugar_expr(rhs, method_resolved);
        }
        HirExpr::Unary { operand, .. } => desugar_expr(operand, method_resolved),
        HirExpr::Deref { expr: e, .. } => desugar_expr(e, method_resolved),
        HirExpr::FieldAccess { object, .. } => desugar_expr(object, method_resolved),
        HirExpr::As { expr: e, .. } => desugar_expr(e, method_resolved),
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                desugar_expr(val, method_resolved);
            }
        }
        HirExpr::EnumVariant { args, .. } => {
            for arg in args {
                desugar_expr(arg, method_resolved);
            }
        }
        HirExpr::TupleLit { elements, .. } => {
            for elem in elements {
                desugar_expr(elem, method_resolved);
            }
        }
        HirExpr::Call { args, .. } => {
            for arg in args {
                desugar_expr(arg, method_resolved);
            }
        }
        HirExpr::Println { arg, .. } => desugar_expr(arg, method_resolved),
        HirExpr::Assert {
            condition, message, ..
        } => {
            desugar_expr(condition, method_resolved);
            if let Some(msg) = message {
                desugar_expr(msg, method_resolved);
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
    use crate::node::{HirBinOp, HirExpr, HirFn};
    use crate::types::{ExprId, HirType};
    use glyim_interner::Interner;
    use std::collections::HashMap;

    fn make_call_expr(
        callee: Symbol,
        recv: HirExpr,
        arg: HirExpr,
        callee_id: ExprId,
    ) -> (Hir, HashMap<ExprId, Symbol>) {
        let mut hir = Hir {
            items: vec![crate::item::HirItem::Fn(HirFn {
                doc: None,
                name: Interner::new().intern("test"),
                type_params: vec![],
                params: vec![],
                param_mutability: vec![],
                ret: None,
                body: HirExpr::MethodCall {
                    id: callee_id,
                    receiver: Box::new(recv),
                    method_name: Interner::new().intern("method"),
                    resolved_callee: None,
                    args: vec![arg],
                    span: glyim_diag::Span::new(0, 0),
                },
                span: glyim_diag::Span::new(0, 0),
                is_pub: false,
                is_macro_generated: false,
                is_extern_backed: false,
            })],
        };
        let mut map = HashMap::new();
        map.insert(callee_id, callee);
        (hir, map)
    }

    #[test]
    fn desugar_method_to_call() {
        let mut interner = Interner::new();
        let callee_sym = interner.intern("my_func");
        let x_sym = interner.intern("x");
        let y_sym = interner.intern("y");
        let callee_id = ExprId::new(0);

        let (mut hir, map) = make_call_expr(
            callee_sym,
            HirExpr::Ident {
                id: ExprId::new(1),
                name: x_sym,
                span: glyim_diag::Span::new(0, 0),
            },
            HirExpr::Ident {
                id: ExprId::new(2),
                name: y_sym,
                span: glyim_diag::Span::new(0, 0),
            },
            callee_id,
        );

        desugar_method_calls(&mut hir, &map);

        let body = match &hir.items[0] {
            crate::item::HirItem::Fn(f) => &f.body,
            _ => panic!("expected Fn"),
        };
        match body {
            HirExpr::Call { callee, args, .. } => {
                assert_eq!(*callee, callee_sym);
                assert_eq!(args.len(), 2);
                assert!(
                    matches!(&args[0], HirExpr::Ident { name, .. } if *name == x_sym),
                    "first arg should be receiver (x)"
                );
                assert!(
                    matches!(&args[1], HirExpr::Ident { name, .. } if *name == y_sym),
                    "second arg should be y"
                );
            }
            other => panic!("Expected Call, got {other:?}"),
        }
    }

    #[test]
    fn desugar_ignored_when_no_map() {
        let mut interner = Interner::new();
        let callee_id = ExprId::new(0);
        let (mut hir, _) = make_call_expr(
            interner.intern("any"),
            HirExpr::IntLit {
                id: ExprId::new(1),
                value: 1,
                span: glyim_diag::Span::new(0, 0),
            },
            HirExpr::IntLit {
                id: ExprId::new(2),
                value: 2,
                span: glyim_diag::Span::new(0, 0),
            },
            callee_id,
        );

        desugar_method_calls(&mut hir, &HashMap::new());

        // should still be MethodCall
        assert!(matches!(
            &hir.items[0],
            crate::item::HirItem::Fn(f) if matches!(&f.body, HirExpr::MethodCall { .. })
        ));
    }
}
