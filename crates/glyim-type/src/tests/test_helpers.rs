//! Internal test helpers for glyim-type tests.
//! We cannot use `glyim_test` crate here because it depends on `glyim_type`,
//! creating a circular dependency that results in multiple crate versions.

use crate::*;
use glyim_core::interner::Interner;

/// Create a fresh mutable type context for testing.
pub fn test_ty_ctx() -> TyCtxMut {
    TyCtxMut::new(Interner::default())
}

/// Run a closure with a fresh `TyCtxMut`, then freeze and return both.
pub fn with_fresh_ty_ctx<F, R>(f: F) -> (TyCtx, R)
where
    F: FnOnce(&mut TyCtxMut) -> R,
{
    let mut ctx_mut = test_ty_ctx();
    let result = f(&mut ctx_mut);
    (ctx_mut.freeze(), result)
}
