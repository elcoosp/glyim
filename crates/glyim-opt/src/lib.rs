//! MIR optimizations (constant propagation, dead code elimination, etc.)
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]

use glyim_mir::Body;
use glyim_type::{TyCtx, TyCtxMut};
use std::sync::Arc;

mod cfg_simplify;
mod constant_prop;
mod dce;
mod slice_desugar;
mod unreachable_elim;
mod validate;

#[derive(Clone, Debug)]
/// Optimized.
pub struct Optimized {
    /// Struct.
    pub body: Body,
}

/// optimize.
pub fn optimize(ctx: &TyCtx, body: &Arc<Body>) -> Optimized {
    let mut body = (**body).clone();
    // Validate well-formedness up front (de-stubbing plan §8.8). Runs
    // unconditionally so release builds also catch any later pass that
    // produces an ill-formed body.
    {
        if let Err(e) = validate::validate_body(ctx, &body) {
            panic!("MIR failed validation before optimization: {:?}", e);
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
        panic!("MIR failed validation after slice_desugar: {:?}", e);
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
