use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::{IntTy, UintTy};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn array_index_to_index_projection() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::Usize));
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

    let mut b = ThirBuilder::new(i32_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding("arr", arr_ty, None, &mut stmts);

    let index_expr = b.expr(
        ExprKind::Index {
            base: Box::new(b.var_ref_expr("arr", arr_ty)),
            index: Box::new(b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty)),
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: index_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
