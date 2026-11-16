//! Report-policy tests for the workspace-diagnostics request
//! (LSP-WORKSPACE-DIAG).
//!
//! Mirrors `tests/pull_diagnostics.rs`: the report policy in
//! [`crate::workspace_diagnostics::workspace_report`] reads the analysed
//! set through the [`crate::workspace_diagnostics::AnalysedSetSource`]
//! seam, so the *composition decision* is pinned here without a database,
//! lock, or driver. The production
//! [`crate::workspace_diagnostics::RecordedAnalysedSource`] and the full
//! wire path are covered end-to-end in `workspace_diagnostics_tests.rs`.

use crate::workspace_diagnostics::{AnalysedFile, AnalysedSetSource, workspace_report};
use glyim_span::FileId;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Uri};
use std::path::PathBuf;
use std::str::FromStr;

struct MockAnalysedSet {
    files: Vec<AnalysedFile>,
}

impl AnalysedSetSource for MockAnalysedSet {
    fn analysed_files(&self) -> Vec<AnalysedFile> {
        self.files.clone()
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

fn analysed(
    file_id: u32,
    name: &str,
    version: Option<i64>,
    diagnostics: Vec<Diagnostic>,
) -> AnalysedFile {
    AnalysedFile {
        file_id: FileId::from_raw(file_id),
        path: PathBuf::from(format!("/test/{name}.g")),
        version,
        diagnostics,
    }
}

fn uri_of(path: &PathBuf) -> Uri {
    lsp_types::Uri::from_str(
        url::Url::from_file_path(path)
            .expect("test paths are absolute")
            .as_str(),
    )
    .expect("test paths convert to a wire URI")
}

#[test]
fn the_workspace_report_carries_one_full_report_per_analysed_file() {
    let mock = MockAnalysedSet {
        files: vec![
            analysed(0, "broken", Some(3), vec![diag("boom")]),
            analysed(1, "clean", None, Vec::new()),
        ],
    };
    let report = workspace_report(&mock);
    assert_eq!(report.items.len(), 2, "one report per analysed file");
    for item in &report.items {
        let lsp_types::WorkspaceDocumentDiagnosticReport::Full(full) = item else {
            panic!("the server never issues result ids; a workspace answer must be full");
        };
        assert_eq!(
            full.full_document_diagnostic_report.result_id, None,
            "the server never issues result ids; a workspace answer must not invent one"
        );
    }
}

#[test]
fn the_workspace_report_sorts_reports_by_ascending_file_id() {
    // The recorded stores are `HashMap`s whose iteration order shifts
    // run to run; the composition decision (ascending file id, i.e.
    // allocation order) lives in the policy so a client polling the
    // workspace sees a stable report order. The mock feeds the files
    // deliberately out of allocation order.
    let mock = MockAnalysedSet {
        files: vec![
            analysed(7, "late", Some(9), Vec::new()),
            analysed(2, "early", Some(4), vec![diag("from-early-file")]),
        ],
    };
    let report = workspace_report(&mock);
    assert_eq!(report.items.len(), 2);
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(first) = &report.items[0] else {
        panic!("a full report");
    };
    assert_eq!(
        first.uri,
        uri_of(&PathBuf::from("/test/early.g")),
        "file id 2 must be reported before file id 7"
    );
}

#[test]
fn the_workspace_report_carries_the_recorded_version_or_none() {
    // `version` is the `didChange` version the driver last analysed at.
    // A file analysed without a version (never opened, restored state)
    // reports `None` — the spec's honest "not marked as open" answer —
    // not a fabricated 0.
    let mock = MockAnalysedSet {
        files: vec![
            analysed(0, "opened", Some(42), Vec::new()),
            analysed(1, "unversioned", None, Vec::new()),
        ],
    };
    let report = workspace_report(&mock);
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(opened) = &report.items[0] else {
        panic!("a full report");
    };
    assert_eq!(opened.version, Some(42), "recorded version passes through");
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(unversioned) = &report.items[1] else {
        panic!("a full report");
    };
    assert_eq!(
        unversioned.version, None,
        "a file with no recorded version must not invent one"
    );
}

#[test]
fn the_workspace_report_preserves_the_recorded_order_within_a_file() {
    // Per-file items pass through in the driver's recording (pipeline)
    // order — the same order `publishDiagnostics` and the per-file pull
    // carry — mirroring the LSP-PULL-DIAG policy pin.
    let mock = MockAnalysedSet {
        files: vec![analysed(
            0,
            "broken",
            Some(1),
            vec![diag("first"), diag("second")],
        )],
    };
    let report = workspace_report(&mock);
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(full) = &report.items[0] else {
        panic!("a full report");
    };
    let items = &full.full_document_diagnostic_report.items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].message, "first");
    assert_eq!(items[1].message, "second");
}

#[test]
fn an_empty_workspace_answers_an_empty_items_array() {
    // The workspace pull is answerable even with nothing analysed: an
    // empty `items` array is the protocol's "no analysed documents"
    // answer, not a failure — unlike the per-file pull, whose `null`
    // (mapped to a JSON-RPC error by the router) means "unknown
    // document".
    let mock = MockAnalysedSet { files: Vec::new() };
    let report = workspace_report(&mock);
    assert!(
        report.items.is_empty(),
        "an empty workspace must still answer, with zero items"
    );
}
