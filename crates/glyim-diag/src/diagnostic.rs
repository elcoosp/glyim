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
    pub fn error(message: impl Into<String>) -> Self { Self { severity: Severity::Error, message: message.into(), span: None } }
    pub fn warning(message: impl Into<String>) -> Self { Self { severity: Severity::Warning, message: message.into(), span: None } }
    pub fn note(message: impl Into<String>) -> Self { Self { severity: Severity::Note, message: message.into(), span: None } }
    pub fn with_span(mut self, span: Span) -> Self { self.span = Some(span); self }
    pub fn with_span_opt(mut self, span: Option<Span>) -> Self { self.span = span; self }
    pub fn is_error(&self) -> bool { self.severity == Severity::Error }
}
