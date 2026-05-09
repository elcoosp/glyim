pub mod biabduction;
pub mod zippering;

use miette::SourceSpan;

/// Convert a glyim_diag::Span to a miette SourceSpan.
pub fn span_to_src(s: glyim_diag::Span) -> SourceSpan {
    (s.start..s.end).into()
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TypeError {
    #[error("infinite type detected")]
    InfiniteType { span: SourceSpan },

    #[error("type mismatch")]
    MismatchedTypes {
        expected_span: SourceSpan,
        found_span: SourceSpan,
        expected: String,
        found: String,
        diff_path: Option<String>,
        autofix: Option<AutoFix>,
    },

    #[error("phase violation: cannot use {used_at} value at {defined_at} stage")]
    PhaseViolation {
        span: SourceSpan,
        used_at: String,
        defined_at: String,
    },

    #[error("const generic mismatch: expected {expected}, found {found}")]
    ConstMismatch {
        span: SourceSpan,
        expected: String,
        found: String,
    },
}

#[derive(Clone, Debug)]
pub enum AutoFix {
    WrapWithOptions(SourceSpan),
    WrapWithOk(SourceSpan),
    TakeAddress(SourceSpan),
}

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> Self {
        let (severity, span, message) = match &err {
            TypeError::InfiniteType { span } => (
                glyim_diag::diagnostic::Severity::Error,
                glyim_diag::Span::new(span.offset(), span.offset() + span.len()),
                err.to_string(),
            ),
            TypeError::MismatchedTypes { expected_span, .. } => (
                glyim_diag::diagnostic::Severity::Error,
                glyim_diag::Span::new(
                    expected_span.offset(),
                    expected_span.offset() + expected_span.len(),
                ),
                err.to_string(),
            ),
            TypeError::PhaseViolation { span, .. } => (
                glyim_diag::diagnostic::Severity::Error,
                glyim_diag::Span::new(span.offset(), span.offset() + span.len()),
                err.to_string(),
            ),
            TypeError::ConstMismatch { span, .. } => (
                glyim_diag::diagnostic::Severity::Error,
                glyim_diag::Span::new(span.offset(), span.offset() + span.len()),
                err.to_string(),
            ),
        };
        glyim_diag::diagnostic::Diagnostic {
            severity,
            file: None,
            span,
            message,
            code: None,
            suggestion: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_error_can_be_converted_to_diagnostic() {
        let err = TypeError::InfiniteType {
            span: (10..20).into(),
        };
        let diag: glyim_diag::diagnostic::Diagnostic = err.into();
        assert!(diag.message.contains("infinite"));
    }
}
