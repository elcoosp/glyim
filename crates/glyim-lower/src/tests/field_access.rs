use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind};

#[test]
fn struct_field_access_does_not_panic() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(Ty::UNIT, interner);
    let mut stmts = Vec::new();
    b.add_let_binding("s", i32_ty, None, &mut stmts);

    let field_name = b.make_name("field");
    let field_expr = b.expr(
        ExprKind::Field {
            receiver: Box::new(b.var_ref_expr("s", i32_ty)),
            field: field_name,
            ty: i32_ty,
        },
        i32_ty,
    );
    stmts.push(thir::Stmt::Expr { expr: field_expr });

    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
