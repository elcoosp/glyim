use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn block_with_tail_produces_value() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    // { let x = 1; x + 2 }  -- block with tail
    let tail_expr = b.expr(
        ExprKind::Binary {
            op: glyim_core::primitives::BinOp::Add,
            lhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty)),
            rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty)),
        },
        i32_ty,
    );
    let block = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: Some(Box::new(tail_expr)),
        },
        i32_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: block }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
    assert!(result.diagnostics.is_empty());
}
