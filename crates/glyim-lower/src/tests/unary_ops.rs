use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::{IntTy, UnOp};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn unary_negation_lowers_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let neg_expr = b.expr(
        ExprKind::Unary {
            op: UnOp::Neg,
            operand: Box::new(b.expr(ExprKind::Literal(Literal::Int(5, None)), i32_ty)),
        },
        i32_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: neg_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn unary_not_lowers_correctly() {
    let ctx_mut = test_ty_ctx();
    let bool_ty = ctx_mut.bool_ty();
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(bool_ty, interner);
    let not_expr = b.expr(
        ExprKind::Unary {
            op: UnOp::Not,
            operand: Box::new(b.expr(ExprKind::Literal(Literal::Bool(true)), bool_ty)),
        },
        bool_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: not_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn unary_deref_lowers_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx_mut.mk_ref(
        glyim_type::Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let deref_expr = b.expr(
        ExprKind::Unary {
            op: UnOp::Deref,
            operand: Box::new(b.expr(ExprKind::Literal(Literal::Int(0, None)), ref_ty)),
        },
        i32_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: deref_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
