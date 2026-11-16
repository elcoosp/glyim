//! Pull-diagnostics request support (feature tag LSP-PULL-DIAG).
//!
//! The server *pushes* diagnostics (`textDocument/publishDiagnostics`,
//! the `diagnostics` engine + analysis driver): every edit republishes
//! the whole set. But a client that polls — a batch test runner, a host
//! that reconnects mid-session, an editor negotiating the 3.17 pull
//! model — had no way to *ask* for the current set:
//! `textDocument/diagnostic` did not exist, even though the authoritative
//! per-file set was already stored in `db.diagnostics`. This module
//! closes that capability gap: the pull answer is the same recorded set
//! the push path publishes, so a client mixing both paths sees identical
//! coordinates and messages.
//!
//! The seam: *which recorded diagnostics a report carries* is split from
//! *how the set is found* by [`DiagnosticSource`], mirroring
//! LSP-IMPLEMENTATION's `ImplSource`. The report policy
//! ([`pull_report`]) reads through the trait and stays testable without
//! a database or lock (`tests/pull_diagnostics.rs`); the production
//! [`RecordedDiagnosticSource`] borrows the `db.diagnostics` map the
//! analysis driver maintains. A future incremental result-id scheme would
//! slot in behind the same seam without touching the policy.
//!
//! Report honesty: the answer is *always* a full report. `previousResultId`
//! is deliberately ignored — unchanged reports are only legal from a
//! server that *issues* result ids, and this one never has; honoring an
//! echoed id would claim a freshness guarantee it cannot keep. Likewise
//! `resultId` stays `None` on the answer. An open, analysed file with no
//! recorded problems answers a full report with zero items — pull
//! semantics has a meaningful empty answer ("checked, nothing to
//! report"), unlike the navigation engines whose "nothing to answer" is
//! `null`. Only a document the server does not know (not open, never
//! analysed) answers `None` via the entry point.
//!
//! Related documents (LSP-RELATED-DIAG): the report carries a
//! `relatedDocuments` entry per file the dependency graph says is
//! *affected by* the pulled one (its transitive dependents — the files
//! the driver re-analyses when it changes), each answering that file's
//! own recorded set through the same pull policy. A file nobody depends
//! on answers without a `relatedDocuments` key at all — the exact shape
//! the pull path answered before the fan-out existed, so clients that
//! never opted in see no change. `inter_file_dependencies: true` was
//! advertised from day one; this is the route that makes it true in
//! fact.
//!
//! Wire mapping (LSP spec): `DocumentDiagnosticReportResult::Report`
//! wrapping a full report with `relatedDocuments` over the affected set
//! (omitted when empty), items in the driver's recorded order — the same
//! order `publishDiagnostics` carries — in the same `SourceMap` UTF-16
//! coordinates the push path uses.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::dep_graph::DependencyGraph;
use glyim_span::FileId;
use lsp_types::{
    Diagnostic, DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportKind,
    DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    RelatedFullDocumentDiagnosticReport,
};
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;

/// The recorded-set seam for pull-diagnostics requests (LSP-PULL-DIAG).
///
/// The report policy in [`pull_report`] must stay testable without a live
/// `AnalysisDatabase` (locks, maps, drivers), so it reads recorded
/// diagnostics through this trait. The production
/// [`RecordedDiagnosticSource`] borrows the driver's map; a future
/// computed-on-demand source would slot in behind the same seam without
/// touching the policy.
pub trait DiagnosticSource {
    /// The diagnostics recorded for `file_id`, in the driver's recording
    /// (pipeline) order — the same order the push path publishes.
    fn diagnostics_of(&self, file_id: FileId) -> Vec<Diagnostic>;
}

/// Production [`DiagnosticSource`]: borrows the driver's recorded
/// `db.diagnostics` map (a missing entry is an empty set — the driver
/// *removes* the entry when a file becomes clean, its documented
/// clearing contract).
///
/// Borrowing (not cloning the map) keeps the request allocation-free
/// apart from the returned items; the read guard lives in the caller.
pub struct RecordedDiagnosticSource<'a> {
    recorded: &'a HashMap<FileId, Vec<Diagnostic>>,
}

impl<'a> RecordedDiagnosticSource<'a> {
    /// Borrows the recorded-diagnostics map as the pull source for one
    /// request.
    #[must_use]
    pub fn new(recorded: &'a HashMap<FileId, Vec<Diagnostic>>) -> Self {
        Self { recorded }
    }
}

impl DiagnosticSource for RecordedDiagnosticSource<'_> {
    fn diagnostics_of(&self, file_id: FileId) -> Vec<Diagnostic> {
        self.recorded.get(&file_id).cloned().unwrap_or_default()
    }
}

/// Pull-diagnostics report policy: the full report for `file_id`.
///
/// Always [`DocumentDiagnosticReportKind::Full`]: `previous_result_id` is
/// ignored (an echoed id must not yield an `unchanged` report — the
/// server never issued one, so it cannot vouch for staleness), and the
/// answer never invents a `resultId`. The recorded set passes through in
/// the driver's order; an empty set is a legal, meaningful empty full
/// report ("checked, nothing to report").
#[must_use]
pub fn pull_report(
    source: &dyn DiagnosticSource,
    file_id: FileId,
    // Ignored on purpose: an echoed id must not yield an `unchanged`
    // report — the server never issued one, so it cannot vouch for
    // staleness. The parameter stays in the signature so a future
    // result-id scheme slots in without touching the callers.
    _previous_result_id: Option<&str>,
) -> DocumentDiagnosticReportKind {
    DocumentDiagnosticReportKind::Full(FullDocumentDiagnosticReport {
        result_id: None,
        items: source.diagnostics_of(file_id),
    })
}

/// `textDocument/diagnostic` request entry point (LSP-PULL-DIAG, with
/// the LSP-RELATED-DIAG fan-out).
///
/// Resolution order: URI → open-file lookup → recorded set through the
/// shared seam → full report → related reports over the dependency
/// graph. Returns `None` only when the document is not one the server
/// knows (not open, never analysed); callers answer `null` for that. An
/// open file with no problems answers an empty full report, not `null`
/// — the empty report is the protocol's "clean" answer. The related
/// documents answer each file affected by the pulled one (its
/// transitive dependents) with that file's own pull answer; a file
/// nobody depends on answers without a `relatedDocuments` key at all.
#[must_use]
pub fn pull_diagnostics(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentDiagnosticParams,
) -> Option<DocumentDiagnosticReportResult> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let recorded = db.diagnostics.read();
    let kind = pull_report(&RecordedDiagnosticSource::new(&recorded), file_id, None);
    let related = {
        let graph = db.dep_graph.read();
        let affected = DependencyGraphSource::new(&graph, file_map);
        related_reports(
            &affected,
            &RecordedDiagnosticSource::new(&recorded),
            file_map,
            file_id,
        )
    };
    drop(recorded);

    let full = match kind {
        DocumentDiagnosticReportKind::Full(full) => full,
        // The policy never answers `unchanged` (pinned by the report
        // tests); this arm exists only for exhaustiveness and degrades to
        // an empty report rather than a panic.
        DocumentDiagnosticReportKind::Unchanged(_) => FullDocumentDiagnosticReport::default(),
    };
    let related_documents = if related.is_empty() {
        None
    } else {
        Some(related)
    };
    tracing::debug!(
        items = full.items.len(),
        related = related_documents.as_ref().map_or(0, |m| m.len()),
        "pull diagnostics answered"
    );
    Some(DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents,
            full_document_diagnostic_report: full,
        }),
    ))
}

// -- Related-document fan-out (LSP-RELATED-DIAG) -------------------------------

/// The affected-set seam for related-document fan-out (LSP-RELATED-DIAG).
///
/// The fan-out policy in [`related_reports`] must stay testable without a
/// live `AnalysisDatabase` (locks, maps, drivers), so it reads the
/// affected set through this trait. The production
/// [`DependencyGraphSource`] borrows the driver's `db.dep_graph` and
/// translates through the session's `FileMap`; a future dependency
/// oracle would slot in behind the same seam without touching the
/// policy.
pub trait AffectedFilesSource {
    /// The ids of the files affected by a change to `origin`, in any
    /// order (the policy imposes the deterministic ascending-`FileId`
    /// order). The origin itself may appear — the policy drops it.
    fn affected_by(&self, origin: FileId) -> Vec<FileId>;
}

/// Production [`AffectedFilesSource`]: borrows the driver's
/// `db.dep_graph` and answers with the transitive reverse-dependency
/// closure of `origin` — the files the driver re-analyses when `origin`
/// changes — translated back to ids through the session's `FileMap`.
/// Files no longer in the `FileMap` (mid-teardown) are skipped: a path
/// without a session id has no URI to key a related report with.
///
/// Borrowing (not cloning the graph) keeps the request
/// allocation-free apart from the returned reports; the read guard
/// lives in the caller.
pub struct DependencyGraphSource<'a> {
    graph: &'a DependencyGraph,
    file_map: &'a FileMap,
}

impl<'a> DependencyGraphSource<'a> {
    /// Borrows the dependency graph and session file map as the
    /// affected-set source for one request.
    #[must_use]
    pub fn new(graph: &'a DependencyGraph, file_map: &'a FileMap) -> Self {
        Self { graph, file_map }
    }
}

impl AffectedFilesSource for DependencyGraphSource<'_> {
    fn affected_by(&self, origin: FileId) -> Vec<FileId> {
        let Some(origin_path) = self.file_map.path(origin) else {
            return Vec::new();
        };
        self.graph
            .affected_files(origin_path)
            .iter()
            .filter_map(|path| self.file_map.get_by_path(path))
            .collect()
    }
}

/// Related-document fan-out policy: one full report per file affected by
/// a change to `origin`, ascending [`FileId`] order, keyed by each
/// file's own URI.
///
/// The origin itself is dropped (its report is the answer, not a
/// related document — [`DependencyGraphSource`]'s traversal seeds the
/// stack with it). Each related report answers through the same
/// [`pull_report`] policy the client-facing report uses, so a client
/// that follows the fan-out sees exactly what a per-file pull would
/// answer. An empty affected set answers an empty map, which the entry
/// point maps to `related_documents: None` (the wire's "no related
/// reports" form).
#[must_use]
pub fn related_reports(
    affected: &dyn AffectedFilesSource,
    recorded: &dyn DiagnosticSource,
    file_map: &FileMap,
    origin: FileId,
) -> HashMap<lsp_types::Uri, DocumentDiagnosticReportKind> {
    let mut ids = affected.affected_by(origin);
    ids.retain(|id| *id != origin);
    // Ascending file id (allocation order): the graph's traversal order
    // is a HashSet walk, so the policy — not the production source —
    // owns the deterministic order.
    ids.sort_by_key(|id| id.to_raw());
    ids.into_iter()
        .filter_map(|id| {
            let path = file_map.path(id)?;
            let wire_uri =
                lsp_types::Uri::from_str(Url::from_file_path(path).ok()?.as_str()).ok()?;
            let kind = pull_report(recorded, id, None);
            // Each related entry is a bare kind (the wire map value):
            // the per-entry `relatedDocuments` recursion is not
            // represented here — one level of fan-out, no nesting.
            Some((wire_uri, kind))
        })
        .collect()
}
