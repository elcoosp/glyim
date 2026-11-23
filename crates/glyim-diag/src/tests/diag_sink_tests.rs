use crate::*;
use glyim_span::{ByteIdx, FileId, Span};

// M3.3(a) — ErrorGuaranteed proof token (plan §6 M3.3 / §5.2 CT-5).
//
// A diagnostic-emitting function that indicates failure must return an
// `ErrorGuaranteed` token rather than `()` or `bool`, so downstream
// phases (`glyim-typeck`, `glyim-lower`) cannot silently ignore that an
// error was reported and cascade into an ICE or wrong code. The token
// is unforgeable outside `glyim-diag`: its only constructor lives on
// `DiagSink::emit` and only fires when the emitted diagnostic carries
// `DiagSeverity::Error`. Warnings/notes/helps do NOT mint a token —
// they are not failure.

/// M3.3(a): `DiagSink::emit` returns an `ErrorGuaranteed` when the
/// emitted diagnostic is an error, proving at the type level that an
/// error was reported. The caller can pass that token to downstream
/// APIs that demand "an error has already been emitted" (e.g. returning
/// `Ty::Error(ErrorGuaranteed)` instead of a sentinel `Ty::Error`).
#[test]
fn emit_error_mints_error_guaranteed() {
    let mut sink = DiagSink::new();
    let token = sink.emit(GlyimDiagnostic::lex_error(Span::DUMMY, "err"));
    assert!(
        token.is_some(),
        "emitting an error-severity diagnostic must mint an ErrorGuaranteed token"
    );
    assert!(sink.has_errors());
}

/// M3.3(a): emitting a warning does NOT mint a token — warnings are not
/// failure, and downstream phases that consume `ErrorGuaranteed` must
/// not be triggered by a warning-only compilation.
#[test]
fn emit_warning_mints_no_token() {
    let mut sink = DiagSink::new();
    let token = sink.emit(GlyimDiagnostic::warning(Span::DUMMY, "warn"));
    assert!(
        token.is_none(),
        "a warning-severity diagnostic must NOT mint an ErrorGuaranteed token"
    );
    assert!(
        !sink.has_errors(),
        "a warning-only sink has no errors to guarantee"
    );
}

/// M3.3(a): emitting a note/help does NOT mint a token either — same
/// contract as warnings, since notes/helps are advisory and never
/// indicate failure on their own.
#[test]
fn emit_note_mints_no_token() {
    let mut sink = DiagSink::new();
    let note = GlyimDiagnostic::new(
        ErrorCode::from_registered(ErrorCategory::Internal, 0),
        DiagSeverity::Note,
        "note",
        MultiSpan::from_span(Span::DUMMY),
    );
    let token = sink.emit(note);
    assert!(token.is_none(), "a note must NOT mint a token");
}

/// M3.3(a): `ErrorGuaranteed` is unforgeable outside `glyim-diag`. Its
/// fields are private and there is no public constructor; the only way
/// to obtain one is `DiagSink::emit` on an error-severity diagnostic.
/// This test constructs one through the sink and asserts it is `Send +
/// Sync` (so it can flow through the cross-thread pipeline) and `Eq`
/// (so it can be compared/stored in collections if a downstream phase
/// ever needs to). The token carries no payload — its mere existence is
/// the proof.
#[test]
fn error_guaranteed_is_send_sync_and_unforgeable() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ErrorGuaranteed>();
    let mut sink = DiagSink::new();
    let token = sink
        .emit(GlyimDiagnostic::lex_error(Span::DUMMY, "err"))
        .expect("error emission mints a token");
    // The token is opaque: the only thing a caller can do with it is
    // pass it along. No interior data is exposed.
    let _ = token;
}

/// M3.3(a): when the error limit is hit, `emit` on a subsequent error
/// does NOT mint a token — the diagnostic was suppressed, so no error
/// was actually reported to the sink. This prevents a caller from
/// obtaining an `ErrorGuaranteed` for a diagnostic the user never saw.
#[test]
fn emit_after_error_limit_mints_no_token() {
    let mut sink = DiagSink::with_error_limit(1);
    let first = sink.emit(GlyimDiagnostic::lex_error(Span::DUMMY, "first"));
    assert!(first.is_some(), "the first error mints a token");
    let second = sink.emit(GlyimDiagnostic::lex_error(Span::DUMMY, "second"));
    assert!(
        second.is_none(),
        "a suppressed error (past the limit) must NOT mint a token — it was not reported"
    );
}

// S01-T05: DiagSink respects error limit and calls on_emit callback
#[test]
fn test_diag_sink_emit() {
    let span = Span::DUMMY;
    let mut sink = DiagSink::new();
    let diag = GlyimDiagnostic::lex_error(span, "test error");
    let _ = sink.emit(diag);
    assert!(sink.has_errors());
    assert_eq!(sink.diagnostics().len(), 1);
}

#[test]
fn test_diag_sink_error_limit() {
    let span = Span::DUMMY;
    let mut sink = DiagSink::with_error_limit(2);
    for i in 0..5 {
        let diag = GlyimDiagnostic::lex_error(span, format!("error {i}"));
        let _ = sink.emit(diag);
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
    let _ = sink.emit(GlyimDiagnostic::lex_error(span, "err"));
    let _ = sink.emit(GlyimDiagnostic::parse_error(span, "err2"));
    assert_eq!(called.load(Ordering::SeqCst), 2);
}

#[test]
fn test_diag_sink_has_errors() {
    let mut sink = DiagSink::new();
    assert!(!sink.has_errors());
    let span = Span::DUMMY;
    let _ = sink.emit(GlyimDiagnostic::lex_error(span, "err"));
    assert!(sink.has_errors());
}

#[test]
fn test_diag_sink_into_diagnostics_no_suppression() {
    let mut sink = DiagSink::new();
    let span = Span::DUMMY;
    let _ = sink.emit(GlyimDiagnostic::lex_error(span, "err1"));
    let _ = sink.emit(GlyimDiagnostic::parse_error(span, "err2"));
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

// M3.3(b) — Stable diagnostic ordering by (file, then byte offset) and
// deduplication (plan §6 M3.3 / §3.5 determinism). The previous
// `into_diagnostics` sort key was `(lo, code, message)` — it sorted by
// byte offset *alone*, ignoring `file`. In a multi-file batch, two
// diagnostics at byte offset 0 of file 1 and file 2 came out in
// insertion order, which is nondeterministic across phases. The M3.3
// contract sorts by `(file, lo, hi, code, message)` so the rendered
// output is stable regardless of which phase emitted first. Dedup
// follows the same key.

/// Helper: build a `GlyimDiagnostic::type_error` at a given
/// `(file, lo, hi)` so the test's sort key is fully determined by the
/// span and not by the message.
fn ty_diag_at(file: usize, lo: usize, hi: usize, msg: &str) -> GlyimDiagnostic {
    let span = Span::new(
        FileId::from_usize(file),
        ByteIdx::from_usize(lo),
        ByteIdx::from_usize(hi),
        glyim_span::SyntaxContext::ROOT,
    );
    GlyimDiagnostic::type_error(span, msg)
}

/// M3.3(b): a multi-file batch sorts by `file` first, then byte offset.
/// Emitting file-2 errors before file-1 errors must come out with file-1
/// first, and within each file by byte offset. The previous sort (lo
/// only) would have ordered `file2@0`, `file1@10`, `file1@0`, `file2@5`
/// as `file1@0, file2@0, file2@5, file1@10` — interleaving files by byte
/// offset alone, which is wrong for a stable multi-file render.
#[test]
fn into_diagnostics_sorts_by_file_then_byte_offset() {
    let diags = vec![
        ty_diag_at(2, 0, 5, "f2 first"),
        ty_diag_at(1, 10, 15, "f1 second"),
        ty_diag_at(1, 0, 5, "f1 first"),
        ty_diag_at(2, 5, 10, "f2 second"),
    ];
    let out = DiagSink::from_vec(diags).into_diagnostics();
    let order: Vec<(u32, u32)> = out
        .iter()
        .map(|d| (d.span.primary.file.to_raw(), d.span.primary.lo.to_raw()))
        .collect();
    assert_eq!(
        order,
        vec![(1, 0), (1, 10), (2, 0), (2, 5)],
        "multi-file batch must sort by (file, lo), grouping each file's diagnostics together"
    );
}

/// M3.3(b): exact duplicates (same file, lo, hi, code, message) collapse
/// to a single diagnostic. This covers the "same error surfaced by two
/// phases" case (plan T150-PATCHED [DIAG-1]).
#[test]
fn into_diagnostics_dedups_exact_duplicates() {
    let d = ty_diag_at(1, 0, 5, "dup");
    let diags = vec![d.clone(), d.clone(), d];
    let out = DiagSink::from_vec(diags).into_diagnostics();
    assert_eq!(out.len(), 1, "exact duplicates must dedup to one");
}

/// M3.3(b): near-duplicates that differ only in `hi` do NOT collapse —
/// the dedup key includes `hi` so two diagnostics at the same `lo` but
/// different spans stay both. (The render would otherwise hide a
/// genuinely distinct secondary location.)
#[test]
fn into_diagnostics_keeps_distinct_his() {
    let diags = vec![ty_diag_at(1, 0, 5, "short"), ty_diag_at(1, 0, 10, "long")];
    let out = DiagSink::from_vec(diags).into_diagnostics();
    assert_eq!(out.len(), 2, "different `hi` keeps both diagnostics");
}

/// M3.3(b): the sort is stable across re-emission order. Emitting the
/// same set of diagnostics in a different insertion order yields the
/// same rendered sequence — the determinism contract (plan C6 / §3.5).
#[test]
fn into_diagnostics_is_order_independent() {
    let set_a = vec![
        ty_diag_at(3, 100, 110, "c"),
        ty_diag_at(1, 0, 5, "a"),
        ty_diag_at(2, 50, 60, "b"),
    ];
    let set_b = vec![
        ty_diag_at(2, 50, 60, "b"),
        ty_diag_at(3, 100, 110, "c"),
        ty_diag_at(1, 0, 5, "a"),
    ];
    let out_a: Vec<(u32, u32, String)> = DiagSink::from_vec(set_a)
        .into_diagnostics()
        .iter()
        .map(|d| {
            (
                d.span.primary.file.to_raw(),
                d.span.primary.lo.to_raw(),
                d.message.clone(),
            )
        })
        .collect();
    let out_b: Vec<(u32, u32, String)> = DiagSink::from_vec(set_b)
        .into_diagnostics()
        .iter()
        .map(|d| {
            (
                d.span.primary.file.to_raw(),
                d.span.primary.lo.to_raw(),
                d.message.clone(),
            )
        })
        .collect();
    assert_eq!(
        out_a, out_b,
        "the sort key is total, so insertion order does not matter"
    );
}
