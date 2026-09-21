//! THIR → MIR lowering + monomorphization.
// Stylistic clippy lints suppressed crate-wide (test-noise lints: cloned_ref_to_slice_refs,
// vec_init_then_push, assertions_on_constants, type_complexity, too_many_arguments, etc.).
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
/// lower_rvalue.
pub mod lower_rvalue;
/// lower_terminator.
pub mod lower_terminator;

pub use lower::*;
// IteratorNextInfo is re-exported via pub use lower::*;
pub use mono::*;
pub mod partition;
pub mod polymorphize;
/// post_mono_checks.
pub mod post_mono_checks;

#[cfg(test)]
mod tests;
