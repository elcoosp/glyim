//! Diagnostic types and source span tracking for the Glyim compiler.

mod diagnostic;
mod span;

pub use diagnostic::{Diagnostic, Severity};
pub use span::Span;
