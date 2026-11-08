use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::{BinOp, IntTy};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn if_inside_while_lowers_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx_mut.bool_ty();
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(0, None)), i32_ty)),
        &mut stmts,
    );

    // while x < 5 { if x == 2 { x = x + 10; } else { x = x + 1; } }
    let inner_if = b.expr(
        ExprKind::If {
            cond: Box::new(b.expr(
                ExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: Box::new(b.var_ref_expr("x", i32_ty)),
                    rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(2, None)), i32_ty)),
                },
                bool_ty,
            )),
            then_branch: Box::new(b.expr(
                ExprKind::Binary {
                    op: BinOp::Add,
                    lhs: Box::new(b.var_ref_expr("x", i32_ty)),
                    rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
                },
                i32_ty,
            )),
            else_branch: Some(Box::new(b.expr(
                ExprKind::Binary {
                    op: BinOp::Add,
                    lhs: Box::new(b.var_ref_expr("x", i32_ty)),
                    rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty)),
                },
                i32_ty,
            ))),
        },
        i32_ty,
    );
    let while_body = b.expr(
        ExprKind::Block {
            stmts: vec![thir::Stmt::Expr { expr: inner_if }],
            tail: None,
        },
        Ty::UNIT,
    );
    let while_expr = b.expr(
        ExprKind::While {
            cond: Box::new(b.expr(
                ExprKind::Binary {
                    op: BinOp::Lt,
                    lhs: Box::new(b.var_ref_expr("x", i32_ty)),
                    rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(5, None)), i32_ty)),
                },
                bool_ty,
            )),
            body: Box::new(while_body),
        },
        Ty::UNIT,
    );
    stmts.push(thir::Stmt::Expr { expr: while_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    // Full lowering produces: entry, while-header, while-body(if-switch),
    // then, else, if-merge, while-exit = 7 blocks
    let _ = assert_mir(&ctx, &result.body).block_count(7);
    assert!(result.diagnostics.is_empty());
}
