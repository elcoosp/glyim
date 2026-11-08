use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

#[test]
fn loop_to_back_edge_block() {
    let ctx_mut = test_ty_ctx();
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(Ty::NEVER, interner);
    let loop_expr = b.expr(
        ExprKind::Loop {
            body: Box::new(b.expr(
                ExprKind::Block {
                    stmts: vec![],
                    tail: None,
                },
                Ty::UNIT,
            )),
        },
        Ty::NEVER,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: loop_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(3);
}
