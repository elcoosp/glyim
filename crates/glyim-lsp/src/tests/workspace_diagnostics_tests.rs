//! Database-backed tests for the workspace-diagnostics request
//! (LSP-WORKSPACE-DIAG).
//!
//! Mirrors `pull_diagnostics_tests.rs`: the real parser populates the
//! database exactly like the analysis driver does — lex/parse diagnostics
//! converted through the same `convert_diagnostics` seam the driver uses,
//! versions recorded exactly like the driver records them — and the
//! public `workspace_diagnostics` entry point is exercised end-to-end
//! (recorded stores → one full report per analysed file). This is also
//! the regression pin for the pull-vs-push contract at the workspace
//! level: a client that polls the whole workspace gets exactly the sets
//! the per-file pull and the push path carry.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::pull_diagnostics::pull_diagnostics;
use crate::workspace_diagnostics::workspace_diagnostics;
use glyim_span::FileId;
use lsp_types::{
    DiagnosticSeverity, DocumentDiagnosticParams, TextDocumentIdentifier, WorkspaceDiagnosticParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> lsp_types::Uri {
    lsp_types::Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// A source with one unmistakable parse error (the `fn` head's `(` never
/// closes) and nothing else interesting.
const BROKEN_DOC: &str = "fn broken( {\n}\n";

/// A source the parser accepts outright.
const CLEAN_DOC: &str = "fn main() -> i32 {\n    42\n}\n";

/// Populate `db` + `file_map` for one document exactly like the driver's
/// analysis step: parse through the real frontend, convert the recorded
/// lex/parse diagnostics through the shared seam, store the LSP set in
/// `db.diagnostics` (the empty set is *removed*, the driver's clearing
/// contract), and record the client version exactly like the driver
/// records it (`None` leaves no version behind).
fn populate(
    db: &AnalysisDatabase,
    file_map: &mut FileMap,
    path: &PathBuf,
    source: &str,
    version: Option<i64>,
) -> FileId {
    let file_id = db.file_map.write().get_or_create(path);
    let caller_file_id = file_map.get_or_create(path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    let sm = SourceMap::new(file_id, source.to_string());
    db.source_maps.write().insert(file_id, sm);
    if let Some(v) = version {
        db.versions.write().insert(file_id, v);
    }
    let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
    let diags = crate::diagnostics::convert_diagnostics(
        file_id,
        db.source_maps.read().get(&file_id).unwrap(),
        &parse_result.diagnostics,
    );
    if diags.is_empty() {
        db.diagnostics.write().remove(&file_id);
    } else {
        db.diagnostics.write().insert(file_id, diags);
    }
    file_id
}

fn workspace_params() -> WorkspaceDiagnosticParams {
    WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

fn params_for(path: &PathBuf) -> DocumentDiagnosticParams {
    DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri: uri_for(path) },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

#[test]
fn workspace_diagnostics_answers_one_report_per_analysed_file() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let broken = PathBuf::from("/test/ws_broken.g");
    let clean = PathBuf::from("/test/ws_clean.g");
    populate(&db, &mut file_map, &broken, BROKEN_DOC, Some(3));
    populate(&db, &mut file_map, &clean, CLEAN_DOC, Some(5));

    let result = workspace_diagnostics(&db, &file_map, &workspace_params());
    let lsp_types::WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("the workspace answer is a full report set, never a partial result");
    };
    assert_eq!(
        report.items.len(),
        2,
        "one report per analysed file, clean files included"
    );
    // Ascending file id (allocation order): the broken file was analysed
    // first and must be reported first, whatever the map iteration order.
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(broken_report) = &report.items[0] else {
        panic!("a full report");
    };
    assert_eq!(
        broken_report.uri,
        uri_for(&broken),
        "the first allocated file is reported first"
    );
    assert!(
        !broken_report
            .full_document_diagnostic_report
            .items
            .is_empty(),
        "the parse error must reach the workspace answer"
    );
    assert_eq!(
        broken_report.version,
        Some(3),
        "the version the driver analysed at passes through"
    );
    let first = &broken_report.full_document_diagnostic_report.items[0];
    assert_eq!(first.source.as_deref(), Some("glyim"));
    assert_eq!(first.severity, Some(DiagnosticSeverity::ERROR));

    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(clean_report) = &report.items[1] else {
        panic!("a full report");
    };
    assert_eq!(clean_report.uri, uri_for(&clean));
    assert!(
        clean_report
            .full_document_diagnostic_report
            .items
            .is_empty(),
        "a clean analysed file answers an empty full report, not null"
    );
    assert_eq!(clean_report.version, Some(5));
}

#[test]
fn workspace_diagnostics_reports_none_for_a_never_versioned_file() {
    // A file analysed without a client version (restored session state)
    // reports `version: None` — the spec's "not marked as open" honesty —
    // not a fabricated 0.
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let clean = PathBuf::from("/test/ws_unversioned.g");
    populate(&db, &mut file_map, &clean, CLEAN_DOC, None);

    let result = workspace_diagnostics(&db, &file_map, &workspace_params());
    let lsp_types::WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("a full report set");
    };
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(clean_report) = &report.items[0] else {
        panic!("a full report");
    };
    assert_eq!(
        clean_report.version, None,
        "no recorded version must not invent one"
    );
}

#[test]
fn workspace_diagnostics_answers_an_empty_workspace_with_no_items() {
    // Nothing analysed yet: the answer is an empty `items` array, not a
    // JSON-RPC error — the workspace pull is always answerable.
    let db = AnalysisDatabase::new();
    let file_map = FileMap::new();

    let result = workspace_diagnostics(&db, &file_map, &workspace_params());
    let lsp_types::WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("a full report set");
    };
    assert!(
        report.items.is_empty(),
        "an empty workspace answers an empty report set"
    );
}

#[test]
fn workspace_diagnostics_sets_match_the_per_file_pull_answer() {
    // Cross-path identity: the workspace report and the per-file pull
    // answer must carry the identical diagnostic set for the same file,
    // or a client mixing both paths shows two different truths.
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let broken = PathBuf::from("/test/ws_parity.g");
    populate(&db, &mut file_map, &broken, BROKEN_DOC, Some(3));

    let result = workspace_diagnostics(&db, &file_map, &workspace_params());
    let lsp_types::WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("a full report set");
    };
    let lsp_types::WorkspaceDocumentDiagnosticReport::Full(workspace_full) = &report.items[0]
    else {
        panic!("a full report");
    };

    let pull = pull_diagnostics(&db, &file_map, &params_for(&broken))
        .expect("an open analysed file always answers a report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(pull_full),
    ) = pull
    else {
        panic!("pull must answer a full report");
    };
    let workspace_items = &workspace_full.full_document_diagnostic_report.items;
    let pull_items = &pull_full.full_document_diagnostic_report.items;
    assert_eq!(workspace_items.len(), pull_items.len(), "same set");
    for (workspace_item, pull_item) in workspace_items.iter().zip(pull_items.iter()) {
        assert_eq!(workspace_item.range, pull_item.range, "same UTF-16 range");
        assert_eq!(workspace_item.message, pull_item.message);
    }
}
