use crate::Span;
use std::path::PathBuf;

/// Severity level for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// A single diagnostic with file location, message, and optional fix.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// Severity of the diagnostic.
    pub severity: Severity,
    /// The file where the issue occurred (None if no file association yet).
    pub file: Option<PathBuf>,
    /// Byte span of the issue in the source file.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
    /// Optional error code (e.g., "E0001").
    pub code: Option<String>,
    /// Optional suggestion for fixing the issue.
    pub suggestion: Option<Suggestion>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Suggestion {
    /// Description of the suggested fix.
    pub message: String,
    /// Replacement text, if any.
    pub replacement: Option<String>,
    /// Span to replace (usually same as diagnostic span).
    pub span: Span,
}

/// Helper to convert a legacy (usize, usize) span into our Span (with no file_id).
pub fn raw_span(start: usize, end: usize) -> Span {
    Span::new(start, end)
}
