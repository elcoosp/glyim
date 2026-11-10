//! MIR optimizations (constant propagation, dead code elimination, etc.)
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

use glyim_mir::Body;
use glyim_type::{TyCtx, TyCtxMut};
use std::sync::Arc;

mod cfg_simplify;
mod constant_prop;
mod dce;
mod slice_desugar;
mod unreachable_elim;
mod validate;

/// The result of a successful [`optimize`] run: the same body with every
/// enabled MIR pass applied.
#[derive(Clone, Debug)]
pub struct Optimized {
    /// The optimized MIR body (a private clone of the input; the caller's
    /// `Arc<Body>` is never mutated).
    pub body: Body,
}

/// Run the full MIR optimization pipeline over `body`.
///
/// Pass order is fixed: `slice_desugar` (with its unconditional
/// post-condition check), `constant_prop`, `dce`, `cfg_simplify`,
/// `unreachable_elim`. The body is validated *before* the first pass, and
/// the `Subslice`-elimination invariant is re-checked right after
/// `slice_desugar`: a violation is a compiler bug (ill-formed MIR handed
/// over by lowering or by a pass), so it surfaces as a precise ICE panic
/// naming the [`MirValidationError`] instead of an opaque failure three
/// passes later (de-stubbing plan §8.7/§8.8).
///
/// # Panics
///
/// Panics when the input body fails [`validate_body`] or when a `Subslice`
/// projection survives `slice_desugar`. Both are compiler bugs, not
/// user-facing errors; the driver's ICE handler renders them.
#[must_use]
pub fn optimize(ctx: &TyCtx, body: &Arc<Body>) -> Optimized {
    let mut body = (**body).clone();
    // Validate well-formedness up front (de-stubbing plan §8.8). Runs
    // unconditionally so release builds also catch any later pass that
    // produces an ill-formed body.
    {
        if let Err(e) = validate::validate_body(ctx, &body) {
            panic!("MIR failed validation before optimization: {e:?}");
        }
    }
    // Runs first and unconditionally: every later pass, and codegen after
    // them, assumes `ConstantIndex`/`Subslice` projections are always
    // terminal (see slice_desugar's module doc). This is a no-op for the
    // (overwhelming majority of) bodies that don't contain any such
    // projection.
    slice_desugar::run(ctx, &mut body);
    // Post-condition (de-stubbing plan §8.7): `slice_desugar` must eliminate
    // every `Subslice` projection. If one survives, codegen's
    // `unreachable!("Subslice")` would fire — catch it here as a precise
    // compiler-bug error instead of three passes later as an opaque panic.
    // Unlike the dev-time `validate_body` check above, this invariant is
    // unconditionally enforced: codegen (glyim-codegen-llvm) treats a
    // surviving `Subslice` as `unreachable!`, so a release build must not
    // silently skip this check and later panic opaquely.
    if let Err(e) = validate::validate_no_subslice(&body) {
        panic!("MIR failed validation after slice_desugar: {e:?}");
    }
    constant_prop::run(ctx, &mut body);
    dce::run(ctx, &mut body);
    cfg_simplify::run(ctx, &mut body);
    unreachable_elim::run(ctx, &mut body);
    Optimized { body }
}

#[cfg(test)]
mod tests;

/// Drop elaboration (de-stubbing plan §8.2). When a `Drop` terminator's
/// operand is definitely initialized, it is replaced with a direct `Drop`;
/// otherwise it is lowered to a `Goto`. Also handles `Drop` on projected
/// places (deref/index/field) by dropping through them rather than skipping.
mod drop_elaboration;

/// Run drop elaboration on the MIR body.
pub fn elaborate_drops(ctx: &mut TyCtxMut, body: &mut Body) {
    drop_elaboration::run(ctx, body);
}

/// Validate MIR well-formedness (de-stubbing plan §8.8). Returns the first
/// invariant violation found, or `Ok(())` if the body is well-formed. See
/// `validate::validate_body` for the full invariant list. Wired as a
/// debug-gated pre-check inside `optimize()` (see the `#[cfg(debug_assertions)]`
/// block there) so it catches ill-formed bodies during development without
/// affecting release codegen.
pub use validate::validate_body;
pub use validate::{MirValidationError, MirValidationErrorKind};
