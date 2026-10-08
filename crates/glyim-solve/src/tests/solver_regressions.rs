//! Solver regression tests for the shapes the audit flagged as
//! un- or under-tested. Each test must FAIL on the pre-fix code to count
//! as a regression test (see the fix-log convention).

use crate::*;
use glyim_core::primitives::{IntTy, Mutability};
use glyim_test::test_ty_ctx;
use glyim_type::*;

/// SOLVE-1: unifying `(a, b)` with `(b, a)` where `a`, `b` are inference
/// vars must terminate. The pre-fix solver could recurse indefinitely on
/// the cyclic binding `a := b, b := a`.
#[test]
fn int_var_cycle_does_not_diverge() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();

    let a = infer.new_int_var(&mut ctx);
    let b = infer.new_int_var(&mut ctx);
    let a_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(a)));
    let b_ty = ctx.mk_ty(TyKind::Infer(InferVar::Int(b)));

    let s1 = ctx.intern_substitution(vec![GenericArg::Ty(a_ty), GenericArg::Ty(b_ty)]);
    let t1 = ctx.mk_tuple(s1);
    let s2 = ctx.intern_substitution(vec![GenericArg::Ty(b_ty), GenericArg::Ty(a_ty)]);
    let t2 = ctx.mk_tuple(s2);

    // The assertion is that this *returns* — Ok or Err both acceptable.
    // A hang or stack overflow fails the test by timeout.
    let _ = infer.unify(&mut ctx, t1, t2, glyim_span::Span::DUMMY);
}

/// `&i32` must not unify with `&str` — a different referent type.
#[test]
fn ref_to_i32_does_not_unify_ref_to_str() {
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let str_ty = ctx.mk_ty(TyKind::String);
    let ref_i32 = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);
    let ref_str = ctx.mk_ref(Region::Erased, str_ty, Mutability::Not);

    let result = infer.unify(&mut ctx, ref_i32, ref_str, glyim_span::Span::DUMMY);
    assert!(
        result.is_err(),
        "&i32 must not unify with &str (different referent types)"
    );
}
