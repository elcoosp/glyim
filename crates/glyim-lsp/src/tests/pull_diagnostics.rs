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

use crate::database::FileMap;
use crate::pull_diagnostics::{DiagnosticSource, pull_report};
use glyim_span::FileId;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use std::collections::HashMap;
use std::path::PathBuf;

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

// -- Related-document fan-out (LSP-RELATED-DIAG) ------------------------------
//
// The same seam discipline as the report policy above: the fan-out
// decision in [`crate::pull_diagnostics::related_reports`] reads the
// affected set through the [`crate::pull_diagnostics::AffectedFilesSource`]
// seam, so the *composition decision* is pinned here without a database,
// lock, or dependency graph. The production
// [`crate::pull_diagnostics::DependencyGraphSource`] and the full wire
// path are covered end-to-end in `pull_diagnostics_tests.rs`.

struct MockAffectedSet {
    affected: Vec<FileId>,
}

impl crate::pull_diagnostics::AffectedFilesSource for MockAffectedSet {
    fn affected_by(&self, _origin: FileId) -> Vec<FileId> {
        self.affected.clone()
    }
}

struct MockSets {
    sets: HashMap<FileId, Vec<Diagnostic>>,
}

impl crate::pull_diagnostics::DiagnosticSource for MockSets {
    fn diagnostics_of(&self, file_id: FileId) -> Vec<Diagnostic> {
        self.sets.get(&file_id).cloned().unwrap_or_default()
    }
}

fn affected(file_id: FileId, name: &str, messages: &[&str]) -> (FileId, PathBuf, Vec<Diagnostic>) {
    (
        file_id,
        PathBuf::from(format!("/test/{name}.g")),
        messages.iter().map(|m| diag(m)).collect(),
    )
}

#[test]
fn related_reports_carry_one_full_kind_per_affected_file() {
    let mut file_map = FileMap::new();
    let origin_path = PathBuf::from("/test/origin.g");
    let dep_path = PathBuf::from("/test/dependent.g");
    // The policy resolves each affected id through the file map — the
    // mock set must carry the ids the map actually allocated, not
    // invented raw values.
    let origin = file_map.get_or_create(&origin_path);
    let dep_id = file_map.get_or_create(&dep_path);
    let (_, _, dep_diags) = affected(dep_id, "dependent", &["boom"]);
    let mock_affected = MockAffectedSet {
        affected: vec![dep_id],
    };
    let mock_sets = MockSets {
        sets: HashMap::from([(dep_id, dep_diags)]),
    };

    let related =
        crate::pull_diagnostics::related_reports(&mock_affected, &mock_sets, &file_map, origin);
    assert_eq!(related.len(), 1, "one report per affected file");
    let (uri, report) = related.iter().next().expect("one entry");
    assert_eq!(
        uri.as_str(),
        url::Url::from_file_path(&dep_path).unwrap().as_str(),
        "the affected file's own URI keys the entry"
    );
    let lsp_types::DocumentDiagnosticReportKind::Full(full) = report else {
        panic!("the server never issues result ids; a related report must be full");
    };
    assert_eq!(full.result_id, None);
    assert_eq!(
        full.items.len(),
        1,
        "the affected file's recorded set passes through"
    );
}

#[test]
fn related_reports_exclude_the_origin_file_itself() {
    // `DependencyGraph::affected_files` includes the origin file (the
    // traversal seeds the stack with it); a related-document entry for
    // the file the client *just pulled* would be redundant — its report
    // is the answer itself.
    let mut file_map = FileMap::new();
    let origin_path = PathBuf::from("/test/origin.g");
    let dep_path = PathBuf::from("/test/dependent.g");
    let origin = file_map.get_or_create(&origin_path);
    let dep_id = file_map.get_or_create(&dep_path);
    let mock_affected = MockAffectedSet {
        affected: vec![origin, dep_id],
    };
    let mock_sets = MockSets {
        sets: HashMap::from([(origin, vec![diag("pulled")]), (dep_id, Vec::new())]),
    };

    let related =
        crate::pull_diagnostics::related_reports(&mock_affected, &mock_sets, &file_map, origin);
    assert_eq!(
        related.len(),
        1,
        "the origin never appears in its own related set"
    );
    let uri = related.keys().next().expect("one entry");
    assert_ne!(
        uri.as_str(),
        url::Url::from_file_path(&origin_path).unwrap().as_str()
    );
}

#[test]
fn related_reports_answer_empty_without_dependents() {
    // No affected files → an empty map, which the entry point maps to
    // `related_documents: None` (the wire's "no related reports" form).
    let mock_affected = MockAffectedSet {
        affected: Vec::new(),
    };
    let mock_sets = MockSets {
        sets: HashMap::new(),
    };
    let file_map = FileMap::new();

    let related = crate::pull_diagnostics::related_reports(
        &mock_affected,
        &mock_sets,
        &file_map,
        FileId::from_raw(0),
    );
    assert!(related.is_empty());
}
