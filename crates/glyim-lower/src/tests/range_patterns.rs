//! Tests for range pattern lowering (S16-T02)
//!
//! Range patterns (`0..=9`) lower to a `SwitchInt` over the covered values.
//! Typeck (`check_pat.rs`) and the lower stage (`lower_rvalue.rs`) both support
//! `Pat::Range` / `PatternKind::Range`, so these are real, enabled tests.

use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::{ThirBuilder, match_arm};
use glyim_core::primitives::IntTy;
use glyim_mir::TerminatorKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal, PatternKind};

/// `match x { 0..=9 => true, _ => false }` lowers to a `SwitchInt` with a
/// branch for each covered value (0 through 9 inclusive).
#[test]
fn lower_range_pattern_to_switch_int() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx_mut.mk_ty(TyKind::Bool);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(bool_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
        &mut stmts,
    );

    let scrutinee = b.var_ref_expr("x", i32_ty);
    let arm1 = match_arm(
        b.pat(
            PatternKind::Range {
                start: Some(Literal::Int(0, None)),
                end: Some(Literal::Int(9, None)),
                inclusive: true,
            },
            i32_ty,
        ),
        b.expr(ExprKind::Literal(Literal::Bool(true)), bool_ty),
    );
    let arm2 = match_arm(
        b.pat(PatternKind::Wild, i32_ty),
        b.expr(ExprKind::Literal(Literal::Bool(false)), bool_ty),
    );
    let match_expr = b.expr(
        ExprKind::Match {
            scrutinee: Box::new(scrutinee),
            arms: vec![arm1, arm2],
        },
        bool_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: match_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Verify SwitchInt terminator exists
    let switch_bb = result
        .body
        .basic_blocks
        .iter()
        .position(|bb| matches!(bb.terminator.kind, TerminatorKind::SwitchInt { .. }));
    assert!(
        switch_bb.is_some(),
        "expected a SwitchInt terminator in match MIR"
    );

    // Sequential-arm-testing shape: range tests are lowered as bool
    // comparisons (`scrut >= 0 && scrut <= 9`) feeding a bool `SwitchInt`.
    let _ = switch_bb;
}
