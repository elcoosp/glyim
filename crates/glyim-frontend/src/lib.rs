//! Frontend: lexer + parser merged.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).

// Plan §3.1 (ratchet, not a hard ban): the frontend consumes untrusted user
// input directly (lexer + parser), so `unwrap`/`expect`/`panic!` on
// source-derived values must eventually become `Diagnostic`/Result. This is a
// `#[warn]` gate (not `#[deny]`) — it surfaces the count under `cargo clippy`
// so the sweep can be tracked and ratcheted down per-PR without breaking the
// build (412 sites won't be fixed in one PR).
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Test code routinely asserts with `unwrap`/`expect`/`panic`. Keep
// the production-code opt-in strict (warn) but allow the same lints
// inside `#[cfg(test)]` modules so test helpers stay concise.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
/// lexer.
pub mod lexer;
/// parser.
pub mod parser;

pub use lexer::{LexResult, Token, lex, lex_with_trivia};
pub use parser::{ParseResult, parse_to_syntax, try_parse_fragment};

#[cfg(test)]
mod tests;
