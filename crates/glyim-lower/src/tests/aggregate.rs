use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn tuple_aggregate_to_rvalue_aggregate() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let subst = ctx_mut.intern_substitution(vec![
        glyim_type::GenericArg::Ty(i32_ty),
        glyim_type::GenericArg::Ty(i32_ty),
    ]);
    let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(subst));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(tuple_ty, interner);
    let agg_expr = b.expr(
        ExprKind::Tuple(vec![
            b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty),
            b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty),
        ]),
        tuple_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: agg_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
