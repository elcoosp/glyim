//! Custom trait solver and type inference engine.
//!
//! [F3+F14] Does NOT define its own `PrintTy`. Imports
//! `glyim_type::PrintTy` which is generic over `TypeLookup`.
//!
//! [F18] Separate `IndexVec`s for `TyVar`, `IntVar`, and
//! `FloatVar`. The type system prevents constructing
//! `InferVar::Int(TyVar(0))` because `Int` takes an `IntVar`.
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

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
