//! Tests for match guard lowering (S16-T03)
//!
//! Match guards are supported end-to-end: typeck (`check_expr.rs`
//! `Expr::Match` lowers `arm.guard` into the THIR `MatchArm::guard`), and the
//! lower stage (`lower_rvalue.rs` `lower_match`) builds a `SwitchInt` on the
//! guard condition to branch into the arm body. These tests exercise that
//! lowering and assert the guard branch is emitted.

use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::{ThirBuilder, match_arm};
use glyim_core::primitives::{BinOp, IntTy};
use glyim_mir::TerminatorKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal, PatternKind};

/// `match x { n if n > 0 => n, _ => 0 }` lowers without diagnostics and emits a
/// guard-branch `SwitchInt` (discriminated on the boolean guard condition).
#[test]
fn lower_guard_branch() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx_mut.mk_ty(TyKind::Bool);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(i32_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
        &mut stmts,
    );

    let scrutinee = b.var_ref_expr("x", i32_ty);

    // Arm: `n if x > 0 => x` (x is the let-bound scrutinee; referencing it from
    // the guard/body avoids needing the pattern-bound `n` to be a named var).
    let n_pat = b.pat(
        PatternKind::Binding {
            var_id: glyim_typeck::thir::LocalVarId::from_raw(0),
            name: b.make_name("n"),
            mutability: glyim_core::primitives::Mutability::Not,
            binding_ty: None,
            subpattern: None,
        },
        i32_ty,
    );
    let x_ref_in_guard = b.var_ref_expr("x", i32_ty);
    let guard_expr = b.expr(
        ExprKind::Binary {
            op: BinOp::Gt,
            lhs: Box::new(x_ref_in_guard),
            rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(0, None)), i32_ty)),
        },
        bool_ty,
    );
    let guarded_body = b.var_ref_expr("x", i32_ty);
    let mut arm1 = match_arm(n_pat, guarded_body);
    arm1.guard = Some(Box::new(guard_expr));

    let arm2 = match_arm(
        b.pat(PatternKind::Wild, i32_ty),
        b.expr(ExprKind::Literal(Literal::Int(0, None)), i32_ty),
    );

    let match_expr = b.expr(
        ExprKind::Match {
            scrutinee: Box::new(scrutinee),
            arms: vec![arm1, arm2],
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: match_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    assert!(
        result.diagnostics.is_empty(),
        "guard match lowering produced diagnostics: {:?}",
        result.diagnostics
    );

    // The guard must be lowered into a `SwitchInt` discriminated on the guard's
    // boolean type (the `n > 0` condition), branching into the arm body.
    let has_guard_switch = result
        .body
        .basic_blocks
        .iter()
        .any(|bb| match &bb.terminator.kind {
            TerminatorKind::SwitchInt { switch_ty, .. } => *switch_ty == bool_ty,
            _ => false,
        });
    assert!(
        has_guard_switch,
        "expected a guard-branch SwitchInt (switch_ty == bool) in match MIR"
    );
}
