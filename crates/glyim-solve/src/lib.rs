//! Custom trait solver and type inference engine.
//!
//! [F3+F14] Does NOT define its own `PrintTy`. Imports
//! `glyim_type::PrintTy` which is generic over `TypeLookup`.
//!
//! [F18] Separate `IndexVec`s for `TyVar`, `IntVar`, and
//! `FloatVar`. The type system prevents constructing
//! `InferVar::Int(TyVar(0))` because `Int` takes an `IntVar`.
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

/// fulfill.
pub mod fulfill;
pub mod hrtb;
/// infer.
pub mod infer;
/// solver.
pub mod solver;

pub use fulfill::{
    FulfillmentCtx, Obligation, ObligationCause, ObligationCauseCode, OverflowError,
};
pub use infer::*;
pub use solver::{
    BuiltinTrait, ImplDef, SimpleTraitSolver, SolverIteratorNextInfo, SolverResult, TraitContext,
    TraitDef, TraitSolver,
};

#[cfg(test)]
mod tests;
