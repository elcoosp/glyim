#![deny(unreachable_patterns)]
pub mod env;
pub mod errors;
pub mod naming;
pub mod solve;
pub mod symbols;
#[cfg(test)]
mod tests;
pub mod typeck;
pub mod unify;
pub mod validate;

pub use errors::{TypeError, UnifyError};
pub use naming::format_type_for_error;
pub use solve::SolveResult;
pub use symbols::KnownSymbols;
pub use typeck::{FnTypes, TypeCheckOutput, TypeCheckResult, TypeChecker};
pub use unify::{ExtractError, ExtractResult, UnificationTable, extract_type_substitutions};
pub use validate::validate_mono_input;

impl From<TypeError> for glyim_diag::diagnostic::Diagnostic {
    fn from(err: TypeError) -> glyim_diag::diagnostic::Diagnostic {
        let (start, end) = match &err {
            TypeError::UnknownField { span, .. } => (span.start, span.end),
            TypeError::MissingField { span, .. } => (span.start, span.end),
            TypeError::NonExhaustiveMatch { span, .. } => (span.start, span.end),
            TypeError::AssignToImmutable { span, .. } => (span.start, span.end),
            TypeError::AssignThroughNonPointer { span, .. } => (span.start, span.end),
            TypeError::DerefNonPointer { span, .. } => (span.start, span.end),
            TypeError::UnresolvedName { span, .. } => (span.start, span.end),
            TypeError::MismatchedTypes { expected_span, .. } => {
                (expected_span.start, expected_span.end)
            }
            TypeError::UnresolvedMethod { span, .. } => (span.start, span.end),
            _ => (0, 0),
        };
        glyim_diag::diagnostic::Diagnostic {
            severity: glyim_diag::diagnostic::Severity::Error,
            file: None,
            span: glyim_diag::Span::new(start, end),
            message: err.to_string(),
            code: None,
            suggestion: None,
        }
    }
}
