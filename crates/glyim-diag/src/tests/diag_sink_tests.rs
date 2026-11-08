use crate::*;
use glyim_span::Span;

// S01-T05: DiagSink respects error limit and calls on_emit callback
#[test]
fn test_diag_sink_emit() {
    let span = Span::DUMMY;
    let mut sink = DiagSink::new();
    let diag = GlyimDiagnostic::lex_error(span, "test error");
    sink.emit(diag);
    assert!(sink.has_errors());
    assert_eq!(sink.diagnostics().len(), 1);
}

#[test]
fn test_diag_sink_error_limit() {
    let span = Span::DUMMY;
    let mut sink = DiagSink::with_error_limit(2);
    for i in 0..5 {
        let diag = GlyimDiagnostic::lex_error(span, format!("error {i}"));
        sink.emit(diag);
    }
    // Only 2 errors recorded, plus an internal error about suppression later
    assert_eq!(sink.diagnostics().len(), 2);
    let diags = sink.into_diagnostics();
    // After into_diagnostics, we get the original 2 plus one suppression note
    assert_eq!(diags.len(), 3);
    assert!(diags.last().unwrap().message.contains("suppressed"));
}

#[test]
fn test_diag_sink_on_emit_callback() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let span = Span::DUMMY;
    let called = Arc::new(AtomicUsize::new(0));
    let called_clone = called.clone();
    let callback = Box::new(move |_diag: &GlyimDiagnostic| {
        called_clone.fetch_add(1, Ordering::SeqCst);
    });
    let mut sink = DiagSink::with_on_emit(Some(callback));
    sink.emit(GlyimDiagnostic::lex_error(span, "err"));
    sink.emit(GlyimDiagnostic::parse_error(span, "err2"));
    assert_eq!(called.load(Ordering::SeqCst), 2);
}

#[test]
fn test_diag_sink_has_errors() {
    let mut sink = DiagSink::new();
    assert!(!sink.has_errors());
    let span = Span::DUMMY;
    sink.emit(GlyimDiagnostic::lex_error(span, "err"));
    assert!(sink.has_errors());
}

#[test]
fn test_diag_sink_into_diagnostics_no_suppression() {
    let mut sink = DiagSink::new();
    let span = Span::DUMMY;
    sink.emit(GlyimDiagnostic::lex_error(span, "err1"));
    sink.emit(GlyimDiagnostic::parse_error(span, "err2"));
    let diags = sink.into_diagnostics();
    assert_eq!(diags.len(), 2);
}

#[test]
fn test_diag_sink_extend() {
    let mut sink = DiagSink::new();
    let span = Span::DUMMY;
    let diags = vec![
        GlyimDiagnostic::lex_error(span, "err1"),
        GlyimDiagnostic::parse_error(span, "err2"),
    ];
    sink.extend(diags);
    assert_eq!(sink.diagnostics().len(), 2);
}
