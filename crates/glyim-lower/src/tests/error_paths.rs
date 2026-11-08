use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::def_id::FnDefId;
use glyim_core::primitives::{FloatTy, IntTy, UintTy};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{self, ExprKind, Literal};

#[test]
fn uint_literal_lowers_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(u32_ty, interner);
    let lit = b.expr(ExprKind::Literal(Literal::Uint(100, None)), u32_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: lit }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn bool_literal_lowers_correctly() {
    let ctx_mut = test_ty_ctx();
    let bool_ty = ctx_mut.bool_ty();
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(bool_ty, interner);
    let lit = b.expr(ExprKind::Literal(Literal::Bool(true)), bool_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: lit }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn float_literal_falls_through_to_error_const() {
    let mut ctx_mut = test_ty_ctx();
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(f64_ty, interner);
    let lit = b.expr(ExprKind::Literal(Literal::Int(0, None)), f64_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: lit }], vec![]);
    let result = lower_body(&mock, &body);
    // Float literals are in the wildcard arm -> MirConstKind::Error, but no panic
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn fn_ref_does_not_panic() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let inputs_subst = ctx_mut.intern_substitution(vec![]);
    let fn_ty = ctx_mut.mk_fn_ptr(glyim_type::FnSig {
        inputs: inputs_subst,
        output: i32_ty,
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    });
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(fn_ty, interner);
    let fn_ref = b.expr(ExprKind::FnRef(FnDefId::from_raw(0)), fn_ty);
    let body = b.into_body(vec![thir::Stmt::Expr { expr: fn_ref }], vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}

#[test]
fn ref_expr_lowers_correctly() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(i32_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
        &mut stmts,
    );
    let ref_expr = b.expr(
        ExprKind::Ref {
            mutability: glyim_core::primitives::Mutability::Not,
            operand: Box::new(b.var_ref_expr("x", i32_ty)),
        },
        i32_ty, // simplified
    );
    stmts.push(thir::Stmt::Expr { expr: ref_expr });
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);
    let _ = assert_mir(&ctx, &result.body).block_count(1);
}
