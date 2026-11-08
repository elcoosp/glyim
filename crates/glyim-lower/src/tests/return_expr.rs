use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn return_with_value_generates_assign_then_return() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(i32_ty, interner);
    let ret_val = b.expr(ExprKind::Literal(Literal::Int(42, None)), i32_ty);
    let body = b.into_body(
        vec![thir::Stmt::Return {
            value: Some(ret_val),
            span: glyim_span::Span::DUMMY,
        }],
        vec![],
    );
    let result = lower_body(&mock, &body);
    // Entry block with assign to _0 then Return
    let _ = assert_mir(&ctx, &result.body).block_count(1);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn return_without_value_just_terminates() {
    let ctx_mut = test_ty_ctx();
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(Ty::UNIT, interner);
    let body = b.into_body(
        vec![thir::Stmt::Return {
            value: None,
            span: glyim_span::Span::DUMMY,
        }],
        vec![],
    );
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
