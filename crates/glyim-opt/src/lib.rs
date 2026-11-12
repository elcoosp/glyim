//! MIR optimizations (constant propagation, dead code elimination, etc.)
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

use glyim_mir::Body;
use glyim_type::TyCtx;
use std::sync::Arc;

mod cfg_simplify;
mod constant_prop;
mod dce;
mod slice_desugar;
mod unreachable_elim;
mod validate;

pub mod pass_plan;

pub use pass_plan::{BuiltinMirPass, MirPass, PassPlan};

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
/// This is the back-compat entry point: it is exactly
/// [`optimize_with_plan`] with the plan [`PassPlan::for_opt_level`] picks
/// for an optimized build, i.e. the pass sequence this crate has always
/// run (`slice_desugar`, `constant_prop`, `dce`, `cfg_simplify`,
/// `unreachable_elim`). Compiles that honor the caller's `--opt-level`
/// go through [`optimize_at_level`] instead.
///
/// # Panics
///
/// Panics when the input body fails [`validate_body`] or when a `Subslice`
/// projection survives `slice_desugar`. Both are compiler bugs, not
/// user-facing errors; the driver's ICE handler renders them.
#[must_use]
pub fn optimize(ctx: &TyCtx, body: &Arc<Body>) -> Optimized {
    optimize_with_plan(ctx, body, &PassPlan::for_opt_level(2))
}

/// Run `plan` over `body`: validate up front, then run each pass in plan
/// order, re-checking the codegen-required `Subslice`-elimination
/// invariant afterwards.
///
/// The validation contract is identical to [`optimize`] and is
/// level-independent by construction: the body is validated *before* the
/// first pass, and [`validate::validate_no_subslice`] runs *after* the
/// plan regardless of its contents — a plan without
/// [`BuiltinMirPass::SliceDesugar`] that meets a body carrying `Subslice`
/// projections surfaces here as a precise compiler-bug panic instead of
/// three stages later in codegen as `unreachable!("Subslice")`.
///
/// # Panics
///
/// Panics when the input body fails [`validate_body`] or when a `Subslice`
/// projection survives the plan (which, for any plan including
/// `slice_desugar`, can only mean a pass reintroduced one).
#[must_use]
pub fn optimize_with_plan(ctx: &TyCtx, body: &Arc<Body>, plan: &PassPlan) -> Optimized {
    let mut body = (**body).clone();
    // Validate well-formedness up front (de-stubbing plan §8.8). Runs
    // unconditionally so release builds also catch any later pass that
    // produces an ill-formed body.
    {
        if let Err(e) = validate::validate_body(ctx, &body) {
            panic!("MIR failed validation before optimization: {e:?}");
        }
    }
    for pass in plan.passes() {
        pass.run(ctx, &mut body);
    }
    // Post-condition (de-stubbing plan §8.7): codegen (glyim-codegen-llvm)
    // treats a surviving `Subslice` as `unreachable!`, so a release build
    // must never hand one over. For plans that include `slice_desugar`
    // this is exactly the check `optimize` has always run right after
    // that pass; for plans that skip it, this is the guard that turns a
    // would-be codegen ICE into the same precise error here.
    if let Err(e) = validate::validate_no_subslice(&body) {
        panic!("MIR failed validation after optimization: {e:?}");
    }
    Optimized { body }
}

/// Run the plan [`PassPlan::for_opt_level`] selects for `opt_level` over
/// `body`.
///
/// `-O0` runs only `slice_desugar` (the pass codegen structurally
/// requires); `-O1`..`-O3` and anything larger run the full pipeline
/// [`optimize`] runs. See [`PassPlan::for_opt_level`] for the level
/// semantics and [`optimize_with_plan`] for the panics.
#[must_use]
pub fn optimize_at_level(ctx: &TyCtx, body: &Arc<Body>, opt_level: u8) -> Optimized {
    optimize_with_plan(ctx, body, &PassPlan::for_opt_level(opt_level))
}

#[cfg(test)]
mod tests;

/// Drop elaboration (de-stubbing plan §8.2). When a `Drop` terminator's
/// operand is definitely initialized, it is replaced with a direct `Drop`;
/// otherwise it is lowered to a `Goto`. Also handles `Drop` on projected
/// places (deref/index/field) by dropping through them rather than skipping.
mod drop_elaboration;

/// Run drop elaboration on the MIR body.
///
/// Also mounted in the pass registry as [`BuiltinMirPass::DropElaboration`]
/// so explicit [`PassPlan`]s can select it; no default opt level includes
/// it (see the `pass_plan` module doc). Takes the same read-only `&TyCtx`
/// receiver every other pass takes — it only queries types and interns
/// the flag-array types it materializes.
pub fn elaborate_drops(ctx: &TyCtx, body: &mut Body) {
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
