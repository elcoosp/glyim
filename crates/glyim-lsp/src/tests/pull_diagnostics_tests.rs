//! Database-backed tests for the pull-diagnostics request
//! (LSP-PULL-DIAG).
//!
//! Mirrors `type_definition_tests.rs`: the real parser populates the
//! database exactly like the analysis driver does — lex/parse diagnostics
//! converted through the same `convert_diagnostics` seam the driver uses —
//! and the public `pull_diagnostics` entry point is exercised end-to-end
//! (URI → open-file lookup → recorded set → full report). This is also the
//! regression pin for the pull-vs-push contract: a client that never
//! receives `publishDiagnostics` (a batch host, a test runner) still gets
//! the same diagnostic set by pulling it.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::diagnostics::convert_diagnostics;
use crate::pull_diagnostics::pull_diagnostics;
use glyim_span::FileId;
use lsp_types::{DiagnosticSeverity, DocumentDiagnosticParams, TextDocumentIdentifier};
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
/// contract).
fn populate(db: &AnalysisDatabase, file_map: &mut FileMap, path: &PathBuf, source: &str) -> FileId {
    let file_id = db.file_map.write().get_or_create(path);
    let caller_file_id = file_map.get_or_create(path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    let sm = SourceMap::new(file_id, source.to_string());
    db.source_maps.write().insert(file_id, sm);
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
fn pull_diagnostics_answers_the_recorded_set_for_a_broken_file() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/pull_broken.g");
    populate(&db, &mut file_map, &path, BROKEN_DOC);

    let result = pull_diagnostics(&db, &file_map, &params_for(&path))
        .expect("an open analysed file always answers a report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("pull must answer a full report");
    };
    assert!(
        !full.full_document_diagnostic_report.items.is_empty(),
        "the parse error must reach the pull answer"
    );
    let first = &full.full_document_diagnostic_report.items[0];
    assert_eq!(first.source.as_deref(), Some("glyim"));
    assert_eq!(first.severity, Some(DiagnosticSeverity::ERROR));
    // The error's range maps into the file (line 0 or 1, inside the text).
    assert!(first.range.start.line <= 1, "range must stay in-document");
    assert!(
        BROKEN_DOC
            .lines()
            .nth(first.range.start.line as usize)
            .is_some(),
        "the reported line must exist in the source"
    );
    assert_eq!(full.related_documents, None, "no related reports yet");
}

#[test]
fn pull_diagnostics_answers_an_empty_full_report_for_a_clean_file() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/pull_clean.g");
    populate(&db, &mut file_map, &path, CLEAN_DOC);

    let result = pull_diagnostics(&db, &file_map, &params_for(&path))
        .expect("an open analysed file always answers a report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("a clean file must still answer a full report");
    };
    assert!(full.full_document_diagnostic_report.items.is_empty());
}

#[test]
fn pull_diagnostics_none_for_an_unknown_document() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/pull_broken.g");
    populate(&db, &mut file_map, &path, BROKEN_DOC);

    let closed = PathBuf::from("/test/never-opened.g");
    assert!(pull_diagnostics(&db, &file_map, &params_for(&closed)).is_none());
}

#[test]
fn pull_diagnostics_positions_match_the_push_mapping() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/pull_broken.g");
    populate(&db, &mut file_map, &path, BROKEN_DOC);

    let result = pull_diagnostics(&db, &file_map, &params_for(&path)).expect("report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("full report");
    };
    // Position identity: the mapping the push path uses (`SourceMap` via
    // `convert_diagnostics`, then publish) is the mapping the pull answer
    // carries, so a client mixing both paths sees identical coordinates
    // for the same problem.
    let sm = SourceMap::new(FileId::from_usize(0), BROKEN_DOC.to_string());
    let parse_result = glyim_frontend::parse_to_syntax(BROKEN_DOC, FileId::from_usize(0));
    let pushed = convert_diagnostics(FileId::from_usize(0), &sm, &parse_result.diagnostics);
    let pulled = &full.full_document_diagnostic_report.items;
    assert_eq!(pushed.len(), pulled.len(), "same set, same order");
    for (push, pull) in pushed.iter().zip(pulled.iter()) {
        assert_eq!(push.range, pull.range, "same UTF-16 range");
        assert_eq!(push.message, pull.message);
    }
}

// -- Related-document fan-out (LSP-RELATED-DIAG) ------------------------------
//
// The dependency graph is populated exactly like the driver populates it
// (`extract_dependencies` records `dependent depends on dependency`), so a
// pull for the *dependency* answers with the *dependents* as related
// documents — the files whose diagnostic sets can change because the pulled
// file changed. A pull for a file with no dependents answers without a
// `relatedDocuments` key at all, exactly like before the fan-out existed.

#[test]
fn pull_diagnostics_fans_out_related_documents_over_the_dependency_graph() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let dependency = PathBuf::from("/test/related_dependency.g");
    let dependent = PathBuf::from("/test/related_dependent.g");
    populate(&db, &mut file_map, &dependency, BROKEN_DOC);
    populate(&db, &mut file_map, &dependent, CLEAN_DOC);
    // The driver's recorded edge shape: `file depends on dep`.
    db.dep_graph
        .write()
        .add_dep(dependent.clone(), dependency.clone());

    let result = pull_diagnostics(&db, &file_map, &params_for(&dependency))
        .expect("an open analysed file always answers a report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("pull must answer a full report");
    };
    let related = full
        .related_documents
        .expect("a pulled dependency carries its dependents as related documents");
    assert_eq!(related.len(), 1, "one related report per affected file");
    let (uri, report) = related.iter().next().expect("one entry");
    assert_eq!(
        uri.as_str(),
        uri_for(&dependent).as_str(),
        "the dependent's URI keys the entry"
    );
    let lsp_types::DocumentDiagnosticReportKind::Full(related_full) = report else {
        panic!("related reports are full reports");
    };
    assert!(
        related_full.items.is_empty(),
        "the dependent is clean: an empty full report, not null"
    );
}

#[test]
fn pull_diagnostics_related_documents_are_transitive_over_dependents() {
    // `affected_files` is the transitive reverse-dep closure: a -> b -> c,
    // so pulling `a` fans out to *both* b and c.
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let a = PathBuf::from("/test/related_a.g");
    let b = PathBuf::from("/test/related_b.g");
    let c = PathBuf::from("/test/related_c.g");
    populate(&db, &mut file_map, &a, BROKEN_DOC);
    populate(&db, &mut file_map, &b, CLEAN_DOC);
    populate(&db, &mut file_map, &c, CLEAN_DOC);
    db.dep_graph.write().add_dep(b.clone(), a.clone());
    db.dep_graph.write().add_dep(c.clone(), b.clone());

    let result = pull_diagnostics(&db, &file_map, &params_for(&a)).expect("report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("full report");
    };
    let related = full.related_documents.expect("transitive dependents");
    assert_eq!(related.len(), 2, "b and c are both affected by a");
    let uris: Vec<&str> = related.keys().map(|uri| uri.as_str()).collect();
    assert!(uris.contains(&uri_for(&b).as_str()), "direct dependent b");
    assert!(
        uris.contains(&uri_for(&c).as_str()),
        "transitive dependent c"
    );
}

#[test]
fn pull_diagnostics_omits_related_documents_without_dependents() {
    // Pulling the *top* of the dependency chain (or any file nobody
    // depends on) answers without a `relatedDocuments` key — the exact
    // shape the pull path answered before the fan-out existed, so
    // clients that never opted in see no change.
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let dependency = PathBuf::from("/test/related_top.g");
    let dependent = PathBuf::from("/test/related_bottom.g");
    populate(&db, &mut file_map, &dependency, BROKEN_DOC);
    populate(&db, &mut file_map, &dependent, CLEAN_DOC);
    db.dep_graph
        .write()
        .add_dep(dependent.clone(), dependency.clone());

    let result = pull_diagnostics(&db, &file_map, &params_for(&dependent))
        .expect("an open analysed file always answers a report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("pull must answer a full report");
    };
    assert_eq!(
        full.related_documents, None,
        "a file with no dependents answers without related documents"
    );
}

#[test]
fn pull_diagnostics_related_sets_match_the_dependents_own_pull_answer() {
    // Cross-path identity, related edition: the related report for a
    // dependent must carry the identical set the dependent's own pull
    // answers with, or a client that follows the fan-out shows a
    // different truth than one that pulls each file.
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let dependency = PathBuf::from("/test/related_parity_dep.g");
    let dependent = PathBuf::from("/test/related_parity_dependent.g");
    populate(&db, &mut file_map, &dependency, BROKEN_DOC);
    populate(&db, &mut file_map, &dependent, BROKEN_DOC);
    db.dep_graph
        .write()
        .add_dep(dependent.clone(), dependency.clone());

    let result = pull_diagnostics(&db, &file_map, &params_for(&dependency)).expect("report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(full),
    ) = result
    else {
        panic!("full report");
    };
    let related = full.related_documents.expect("the dependent is affected");
    let (_, report) = related.iter().next().expect("one entry");
    let lsp_types::DocumentDiagnosticReportKind::Full(related_full) = report else {
        panic!("related reports are full reports");
    };

    let own = pull_diagnostics(&db, &file_map, &params_for(&dependent)).expect("report");
    let lsp_types::DocumentDiagnosticReportResult::Report(
        lsp_types::DocumentDiagnosticReport::Full(own_full),
    ) = own
    else {
        panic!("full report");
    };
    let related_items = &related_full.items;
    let own_items = &own_full.full_document_diagnostic_report.items;
    assert_eq!(related_items.len(), own_items.len(), "same set");
    for (related_item, own_item) in related_items.iter().zip(own_items.iter()) {
        assert_eq!(related_item.range, own_item.range, "same UTF-16 range");
        assert_eq!(related_item.message, own_item.message);
    }
}
