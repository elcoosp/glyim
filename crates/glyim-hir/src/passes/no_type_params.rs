//! Pass that asserts no unresolved type parameters remain in the HIR after monomorphization.
//! Unresolved params are single‑uppercase‑letter symbols (e.g., T, K, V).

use crate::node::{HirExpr, HirStmt};
use crate::types::HirType;
use glyim_interner::Interner;

/// Check if a type contains an unresolved type parameter.
pub fn has_unresolved_param(ty: &HirType, interner: &Interner) -> bool {
    match ty {
        HirType::Named(sym) => {
            let s = interner.resolve(*sym);
            s.len() == 1 && s.chars().next().map_or(false, |c| c.is_uppercase())
        }
        HirType::Generic(_, args) => args.iter().any(|a| has_unresolved_param(a, interner)),
        HirType::RawPtr(inner) => has_unresolved_param(inner, interner),
        HirType::Option(inner) => has_unresolved_param(inner, interner),
        HirType::Result(ok, err) => {
            has_unresolved_param(ok, interner) || has_unresolved_param(err, interner)
        }
        HirType::Tuple(elems) => elems.iter().any(|e| has_unresolved_param(e, interner)),
        _ => false,
    }
}

/// Panics if any expression or statement in the function body contains an unresolved type parameter.
pub fn assert_no_type_params(expr: &HirExpr, interner: &Interner) {
    match expr {
        HirExpr::IntLit { .. }
        | HirExpr::FloatLit { .. }
        | HirExpr::BoolLit { .. }
        | HirExpr::StrLit { .. }
        | HirExpr::Ident { .. }
        | HirExpr::UnitLit { .. }
        | HirExpr::AddrOf { .. } => {}

        HirExpr::Binary { lhs, rhs, .. } => {
            assert_no_type_params(lhs, interner);
            assert_no_type_params(rhs, interner);
        }
        HirExpr::Unary { operand, .. }
        | HirExpr::Deref { expr: operand, .. } => {
            assert_no_type_params(operand, interner);
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { value, .. }
                    | HirStmt::LetPat { value, .. }
                    | HirStmt::Assign { value, .. }
                    | HirStmt::AssignField { value, .. } => {
                        assert_no_type_params(value, interner);
                    }
                    HirStmt::AssignDeref { target, value, .. } => {
                        assert_no_type_params(target, interner);
                        assert_no_type_params(value, interner);
                    }
                    HirStmt::Expr(e) => assert_no_type_params(e, interner),
                }
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            assert_no_type_params(condition, interner);
            assert_no_type_params(then_branch, interner);
            if let Some(e) = else_branch {
                assert_no_type_params(e, interner);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            assert_no_type_params(scrutinee, interner);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    assert_no_type_params(g, interner);
                }
                assert_no_type_params(body, interner);
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
            assert_no_type_params(condition, interner);
            assert_no_type_params(body, interner);
        }
        HirExpr::Call { args, .. } => {
            for a in args {
                assert_no_type_params(a, interner);
            }
        }
        HirExpr::MethodCall {
            receiver, args, ..
        } => {
            assert_no_type_params(receiver, interner);
            for a in args {
                assert_no_type_params(a, interner);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, val) in fields {
                assert_no_type_params(val, interner);
            }
        }
        HirExpr::EnumVariant { args, .. }
        | HirExpr::TupleLit { elements: args, .. } => {
            for a in args {
                assert_no_type_params(a, interner);
            }
        }
        HirExpr::Println { arg, .. } => assert_no_type_params(arg, interner),
        HirExpr::Assert {
            condition, message, ..
        } => {
            assert_no_type_params(condition, interner);
            if let Some(m) = message {
                assert_no_type_params(m, interner);
            }
        }
        HirExpr::Return { value, .. } => {
            if let Some(v) = value {
                assert_no_type_params(v, interner);
            }
        }
        HirExpr::SizeOf { target_type, .. }
        | HirExpr::As {
            target_type, ..
        } => {
            assert!(
                !has_unresolved_param(target_type, interner),
                "Unresolved type parameter in type: {:?}",
                target_type
            );
        }
        _ => {}
    }
}
