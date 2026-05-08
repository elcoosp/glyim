pub mod zippering;
pub mod biabduction;

use miette::SourceSpan;

/// Convert our Span to SourceSpan.
/// Convert a glyim_diag::Span to a miette SourceSpan.
pub fn span_to_src(s: glyim_diag::Span) -> SourceSpan {
    (s.start..s.end).into()
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TypeError {
    #[error("infinite type detected")]
    InfiniteType {
        span: SourceSpan,
    },

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
