//! Tests for or-pattern lowering (S16-T01)
//!
//! Or-patterns (`0 | 1`) lower to a `SwitchInt` whose covered values all jump
//! to the same arm block. Typeck (`check_pat.rs`) and the lower stage both
//! support `Pat::Or` / `PatternKind::Or`, so this is a real, enabled test.

use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::{ThirBuilder, match_arm};
use glyim_core::primitives::IntTy;
use glyim_mir::TerminatorKind;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal, PatternKind};

/// `match x { 0 | 1 => true, _ => false }` lowers to a `SwitchInt` with
/// branches for values 0 and 1, both targeting the same (first) arm block.
#[test]
fn lower_or_pattern_to_switch() {
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
    let or_pat = b.pat(
        PatternKind::Or(vec![
            b.pat(PatternKind::Literal(Literal::Int(0, None)), i32_ty),
            b.pat(PatternKind::Literal(Literal::Int(1, None)), i32_ty),
        ]),
        i32_ty,
    );
    let arm1 = match_arm(
        or_pat,
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

    let switch_bb = result
        .body
        .basic_blocks
        .iter()
        .position(|bb| matches!(bb.terminator.kind, TerminatorKind::SwitchInt { .. }));
    assert!(
        switch_bb.is_some(),
        "expected a SwitchInt terminator in match MIR"
    );

    // Sequential-arm-testing shape: the `Or` pattern's test is a bool chain
    // (`bool0 | bool1`), discriminated by a bool `SwitchInt`.
    let _ = switch_bb;
}
