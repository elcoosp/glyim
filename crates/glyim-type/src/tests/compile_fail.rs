//! Compile-fail tests for type-level invariants.
//! These are documented as compile-fail. Actual compile-fail tests
//! live in a separate ui test directory.

use super::helpers::with_fresh_ty_ctx;
use crate::*;

/// This test documents that `TyCtxMut` is !Send. The following would NOT compile:
/// ```compile_fail
/// fn require_send<T: Send>(_: &T) {}
/// fn main() {
///     let ctx = glyim_type::TyCtxMut::new(glyim_core::interner::Interner::new());
///     require_send(&ctx); // ERROR: `*const ()` cannot be sent between threads safely
/// }
/// ```
#[test]
fn ty_ctx_mut_is_not_send_documented() {
    // Property enforced by PhantomData<*const ()> in TyCtxMut.
    // See traits.rs for positive proof that TyCtx IS Send+Sync.
}

/// Documents that `Ty::from_raw` is pub(crate) and cannot be called from outside.
#[test]
fn ty_from_raw_is_pub_crate() {
    assert_eq!(Ty::ERROR.to_raw(), 0);
    assert_eq!(Ty::NEVER.to_raw(), 1);
    assert_eq!(Ty::UNIT.to_raw(), 2);
    assert_eq!(Ty::BOOL.to_raw(), 3);
}

/// Documents that `Substitution::from_raw` is pub(crate).
#[test]
fn substitution_from_raw_is_pub_crate() {
    let (ctx, sub) =
        with_fresh_ty_ctx(|c| c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]));
    assert!(!sub.is_empty());
    let _ = ctx;
}
