//! Report-policy tests for the pull-diagnostics request
//! (LSP-PULL-DIAG).
//!
//! Mirrors `tests/implementation.rs`: the report policy in
//! [`crate::pull_diagnostics::pull_report`] reads recorded diagnostics
//! through the [`crate::pull_diagnostics::DiagnosticSource`] seam, so the
//! *report-shape decision* is pinned here without a database, lock, or
//! driver. The production [`crate::pull_diagnostics::RecordedDiagnosticSource`]
//! and the full wire path are covered end-to-end in
//! `pull_diagnostics_tests.rs`.

use crate::pull_diagnostics::{DiagnosticSource, pull_report};
use glyim_span::FileId;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

struct MockSource {
    recorded: Vec<Diagnostic>,
}

impl DiagnosticSource for MockSource {
    fn diagnostics_of(&self, _file_id: FileId) -> Vec<Diagnostic> {
        self.recorded.clone()
    }
}

fn diag(message: &str) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position {
                line: 3,
                character: 0,
            },
            end: Position {
                line: 3,
                character: 7,
            },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("glyim".to_string()),
        message: message.to_string(),
        ..Default::default()
    }
}

#[test]
fn the_report_is_always_full_and_carries_the_recorded_items() {
    let mock = MockSource {
        recorded: vec![diag("first"), diag("second")],
    };
    let report = pull_report(&mock, FileId::from_raw(0), None);
    let lsp_types::DocumentDiagnosticReportKind::Full(full) = report else {
        panic!("pull must answer a full report");
    };
    assert_eq!(
        full.result_id, None,
        "the server never issues result ids; a pull answer must not invent one"
    );
    assert_eq!(full.items.len(), 2);
    // The recorded (pipeline) order passes through untouched: lex/parse
    // diagnostics first is the driver's recording order and the push
    // engine's publish order — the pull answer must not re-sort it.
    assert_eq!(full.items[0].message, "first");
    assert_eq!(full.items[1].message, "second");
}

#[test]
fn a_client_echoed_result_id_is_never_honored() {
    // `previousResultId` exists so a server that *issues* result ids can
    // answer `unchanged`. This server never issues one, so honoring an
    // echoed id would claim a freshness guarantee it cannot keep — the
    // policy answers a full report instead, whatever the client echoes.
    let mock = MockSource {
        recorded: vec![diag("boom")],
    };
    let report = pull_report(&mock, FileId::from_raw(0), Some("stale-id"));
    let lsp_types::DocumentDiagnosticReportKind::Full(_) = report else {
        panic!("an echoed result id must not yield an unchanged report");
    };
}

#[test]
fn a_clean_analysed_file_answers_an_empty_full_report() {
    // Pull diagnostics has a meaningful empty answer: a full report with
    // zero items says "checked, nothing to report". Null would read as a
    // protocol failure, and an unchanged report would need a result id.
    let mock = MockSource {
        recorded: Vec::new(),
    };
    let report = pull_report(&mock, FileId::from_raw(0), None);
    let lsp_types::DocumentDiagnosticReportKind::Full(full) = report else {
        panic!("a clean file must still answer a full report");
    };
    assert!(full.items.is_empty());
}
