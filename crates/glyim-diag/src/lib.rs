mod diagnostic;
mod render;
mod span;
pub use diagnostic::{Diagnostic, Severity};
pub use render::{render_diagnostics, render_single};
pub use span::Span;
