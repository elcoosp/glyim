//! Frontend: lexer + parser merged.
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
// Plan §3.1 (ratchet, not a hard ban): the frontend consumes untrusted user
// input directly (lexer + parser), so `unwrap`/`expect`/`panic!` on
// source-derived values must eventually become `Diagnostic`/Result. This is a
// `#[warn]` gate (not `#[deny]`) — it surfaces the count under `cargo clippy`
// so the sweep can be tracked and ratcheted down per-PR without breaking the
// build (412 sites won't be fixed in one PR).
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
/// lexer.
pub mod lexer;
/// parser.
pub mod parser;

pub use lexer::{LexResult, Token, lex};
pub use parser::{ParseResult, parse_to_syntax, try_parse_fragment};

#[cfg(test)]
mod tests;
