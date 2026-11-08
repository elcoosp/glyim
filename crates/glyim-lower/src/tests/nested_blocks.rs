use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn deeply_nested_blocks_lower_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    // { { { 42 } } }
    let inner = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: Some(Box::new(
                b.expr(ExprKind::Literal(Literal::Int(42, None)), i32_ty),
            )),
        },
        i32_ty,
    );
    let mid = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: Some(Box::new(inner)),
        },
        i32_ty,
    );
    let outer = b.expr(
        ExprKind::Block {
            stmts: vec![],
            tail: Some(Box::new(mid)),
        },
        i32_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: outer }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn block_with_multiple_statements() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "a",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty)),
        &mut stmts,
    );
    b.add_let_binding(
        "b",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty)),
        &mut stmts,
    );
    let sum = b.expr(
        ExprKind::Binary {
            op: glyim_core::primitives::BinOp::Add,
            lhs: Box::new(b.var_ref_expr("a", i32_ty)),
            rhs: Box::new(b.var_ref_expr("b", i32_ty)),
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: sum });
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
