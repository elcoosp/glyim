use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::{BinOp, IntTy};
use glyim_mir::BasicBlockIdx;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn while_to_loop_with_conditional_break() {
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
        Some(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
        &mut stmts,
    );

    let cond = b.expr(
        ExprKind::Binary {
            op: BinOp::Gt,
            lhs: Box::new(b.var_ref_expr("x", i32_ty)),
            rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(0, None)), i32_ty)),
        },
        bool_ty,
    );
    let body_stmt = thir::Stmt::Expr {
        expr: b.expr(
            ExprKind::Binary {
                op: BinOp::Sub,
                lhs: Box::new(b.var_ref_expr("x", i32_ty)),
                rhs: Box::new(b.expr(ExprKind::Literal(Literal::Int(1, None)), i32_ty)),
            },
            i32_ty,
        ),
    };
    let body_block = b.expr(
        ExprKind::Block {
            stmts: vec![body_stmt],
            tail: None,
        },
        Ty::UNIT,
    );
    let while_expr = b.expr(
        ExprKind::While {
            cond: Box::new(cond),
            body: Box::new(body_block),
        },
        Ty::UNIT,
    );
    stmts.push(thir::Stmt::Expr { expr: while_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body)
        .block_count(4)
        .block_terminator(BasicBlockIdx::from_raw(0), "Goto");
}
