use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn cast_expr_to_rvalue_cast() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i64_ty, interner);
    let cast_expr = b.expr(
        ExprKind::Cast {
            expr: Box::new(b.expr(ExprKind::Literal(Literal::Int(42, None)), i32_ty)),
        },
        i64_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: cast_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
