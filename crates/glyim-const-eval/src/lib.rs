//! Const evaluator for the Glyim compiler.
//!
//! This crate provides a HIR-based constant evaluator that supports
//! literals, arithmetic, `if` expressions, and `match` expressions.
//! It is called during HIR→THIR lowering to replace `ConstBlock`
//! patterns with evaluated literals.

pub mod eval;
pub mod value;

pub use eval::{BodyFn, ConstEvaluator};
pub use value::ConstValue;

/// Maximum recursion depth for const evaluation.
pub const MAX_EVAL_DEPTH: u32 = 128;

/// Error produced during constant evaluation.
#[derive(Debug, Clone)]
pub struct ConstEvalError {
    /// Human-readable error message.
    pub message: String,
    /// Source span where the error occurred.
    pub span: glyim_span::Span,
}

impl ConstEvalError {
    /// Create a new const evaluation error.
    pub fn new(message: impl Into<String>, span: glyim_span::Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// Convert this error into a diagnostic.
    #[must_use]
    pub fn into_diagnostic(self) -> glyim_diag::GlyimDiagnostic {
        use glyim_diag::{DiagSeverity, ErrorCategory, ErrorCode};
        glyim_diag::GlyimDiagnostic::new(
            ErrorCode {
                category: ErrorCategory::Comptime,
                number: 1,
            },
            DiagSeverity::Error,
            format!("const evaluation error: {}", self.message),
            self.span.into(),
        )
    }
}

/// Result of a constant evaluation.
pub type ConstEvalResult<T> = Result<T, ConstEvalError>;

#[cfg(test)]
mod tests;
