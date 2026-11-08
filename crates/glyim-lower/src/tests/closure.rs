use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

#[test]
fn closure_lowering_does_not_panic() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let closure_body = thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![],
        return_ty: i32_ty,
        stmts: vec![],
        span: glyim_span::Span::DUMMY,
    };
    let closure_expr = b.expr(
        ExprKind::Closure {
            body: Box::new(closure_body),
            captures: vec![],
            is_move: false,
        },
        i32_ty,
    );
    let body = b.into_body(vec![thir::Stmt::Expr { expr: closure_expr }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
