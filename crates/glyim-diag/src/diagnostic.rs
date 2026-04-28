//! Compiler diagnostic with optional source span.

use crate::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity { Error, Warning, Note }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: message.into(), span: None }
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, message: message.into(), span: None }
    }
    pub fn note(message: impl Into<String>) -> Self {
        Self { severity: Severity::Note, message: message.into(), span: None }
    }
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
    pub fn is_error(&self) -> bool { self.severity == Severity::Error }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn error_diagnostic() {
        let d = Diagnostic::error("something broke");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, "something broke");
        assert!(d.span.is_none());
        assert!(d.is_error());
    }
    #[test] fn warning_diagnostic() {
        let d = Diagnostic::warning("deprecated");
        assert_eq!(d.severity, Severity::Warning);
        assert!(!d.is_error());
    }
    #[test] fn note_diagnostic() {
        let d = Diagnostic::note("see also: ...");
        assert_eq!(d.severity, Severity::Note);
        assert!(!d.is_error());
    }
    #[test] fn with_span_attaches_span() {
        let d = Diagnostic::error("oops").with_span(Span::new(10, 20));
        assert_eq!(d.span, Some(Span::new(10, 20)));
    }
    #[test] fn with_span_does_not_mutate_original() {
        let d1 = Diagnostic::error("oops");
        let d2 = d1.clone().with_span(Span::new(0, 5));
        assert!(d1.span.is_none(), "original should have no span");
        assert!(d2.span.is_some(), "clone with span should have span");
    }
}
