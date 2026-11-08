//! THIR → MIR lowering + monomorphization.
// Stylistic clippy lints suppressed crate-wide (test-noise lints: cloned_ref_to_slice_refs,
// vec_init_then_push, assertions_on_constants, type_complexity, too_many_arguments, etc.).

/// discovery.
pub mod discovery;
/// lower.
pub mod lower;
pub mod mono;

// New split modules (private to crate, exposed via lower)
/// async_state_transform.
pub mod async_state_transform;
/// builder.
pub mod builder;
/// `lower_rvalue`.
pub mod lower_rvalue;
/// `lower_terminator`.
pub mod lower_terminator;

pub use lower::*;
// IteratorNextInfo is re-exported via pub use lower::*;
pub use mono::*;
pub mod partition;
pub mod polymorphize;
/// `post_mono_checks`.
pub mod post_mono_checks;

#[cfg(test)]
mod tests;
