use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn array_literal_lowers_to_aggregate() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize));
    let arr_ty = ctx_mut.mk_ty(TyKind::Array(
        i32_ty,
        glyim_type::Const {
            kind: glyim_type::ConstKind::Int(3),
            ty: usize_ty,
        },
    ));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(arr_ty, interner);
    let arr_expr = b.expr(
        ExprKind::Array(vec![
            b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
            b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty),
            b.expr(ExprKind::Literal(Literal::Int(3, None)), i32_ty),
        ]),
        arr_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: arr_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
