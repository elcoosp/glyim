use crate::*;
use glyim_span::{ByteIdx, FileId, MultiSpan, Span};
use std::sync::Arc;

// S01-T04: GlyimDiagnostic builders create correct severity and code
#[test]
fn test_lex_error_constructor() {
    let span = Span::new(
        FileId::from_raw(1),
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        glyim_span::SyntaxContext::ROOT,
    );
    let diag = GlyimDiagnostic::lex_error(span, "invalid token");
    assert_eq!(diag.code.category, ErrorCategory::Lex);
    assert_eq!(diag.severity, DiagSeverity::Error);
    assert_eq!(diag.message, "invalid token");
    assert_eq!(diag.span.primary, span);
}

#[test]
fn test_parse_error_constructor() {
    let span = Span::DUMMY;
    let diag = GlyimDiagnostic::parse_error(span, "missing semicolon");
    assert_eq!(diag.code.category, ErrorCategory::Parse);
    assert_eq!(diag.severity, DiagSeverity::Error);
    assert_eq!(diag.message, "missing semicolon");
}

#[test]
fn test_type_error_constructor() {
    let span = Span::DUMMY;
    let diag = GlyimDiagnostic::type_error(span, "mismatched types");
    assert_eq!(diag.code.category, ErrorCategory::Type);
}

#[test]
fn test_borrow_error_constructor() {
    let span = Span::DUMMY;
    let diag = GlyimDiagnostic::borrow_error(span, "cannot move out of borrowed content");
    assert_eq!(diag.code.category, ErrorCategory::Borrow);
}

#[test]
fn test_internal_error_constructor() {
    let diag = GlyimDiagnostic::internal_error("internal assertion failed");
    assert_eq!(diag.code.category, ErrorCategory::Internal);
    assert_eq!(diag.span.primary, Span::DUMMY);
}

#[test]
fn test_with_source_code() {
    let span = Span::DUMMY;
    let diag =
        GlyimDiagnostic::parse_error(span, "error").with_source_code(Arc::from("source code"));
    assert_eq!(diag.source_code, Some(Arc::from("source code")));
}

#[test]
fn test_with_sub() {
    let span = Span::DUMMY;
    let sub = SubDiagnostic {
        severity: DiagSeverity::Note,
        message: "note message".to_string(),
        span: None,
    };
    let diag = GlyimDiagnostic::parse_error(span, "error").with_sub(sub.clone());
    assert_eq!(diag.sub_diagnostics.len(), 1);
    assert_eq!(diag.sub_diagnostics[0].message, "note message");
}

#[test]
fn test_with_suggestion() {
    let span = Span::DUMMY;
    let sug = Suggestion {
        message: "try adding a semicolon".to_string(),
        replacements: vec![(span, ";".to_string())],
        applicability: Applicability::MachineApplicable,
    };
    let diag = GlyimDiagnostic::parse_error(span, "error").with_suggestion(sug.clone());
    assert_eq!(diag.suggestions.len(), 1);
    assert_eq!(diag.suggestions[0].message, "try adding a semicolon");
}

#[test]
fn test_is_error() {
    let span = Span::DUMMY;
    let err = GlyimDiagnostic::lex_error(span, "error");
    assert!(err.is_error());
    let warn = GlyimDiagnostic::new(
        ErrorCode {
            category: ErrorCategory::Internal,
            number: 0,
        },
        DiagSeverity::Warning,
        "warning",
        MultiSpan::from_span(span),
    );
    assert!(!warn.is_error());
}

#[test]
fn test_display_implementation() {
    let span = Span::DUMMY;
    let diag = GlyimDiagnostic::lex_error(span, "bad token");
    let output = format!("{}", diag);
    assert!(output.contains("E0001"));
    assert!(output.contains("bad token"));
}

#[test]
fn test_error_trait() {
    let span = Span::DUMMY;
    let diag = GlyimDiagnostic::lex_error(span, "test");
    let _err: &dyn std::error::Error = &diag; // must compile
}

#[test]
fn warning_severity_test() {
    use glyim_span::Span;
    let d = GlyimDiagnostic::warning(Span::DUMMY, "test warning message");
    assert!(matches!(d.severity, DiagSeverity::Warning));
    assert!(!d.is_error());
    assert_eq!(d.message, "test warning message");
}
