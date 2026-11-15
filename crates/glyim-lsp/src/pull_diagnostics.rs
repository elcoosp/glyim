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
//! Wire mapping (LSP spec): `DocumentDiagnosticReportResult::Report`
//! wrapping a full report with `relatedDocuments: None` (no cross-file
//! report fan-out yet), items in the driver's recorded order — the same
//! order `publishDiagnostics` carries — in the same `SourceMap` UTF-16
//! coordinates the push path uses.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_span::FileId;
use lsp_types::{
    Diagnostic, DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportKind,
    DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    RelatedFullDocumentDiagnosticReport,
};
use std::collections::HashMap;
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

/// `textDocument/diagnostic` request entry point (LSP-PULL-DIAG).
///
/// Resolution order: URI → open-file lookup → recorded set through the
/// shared seam → full report. Returns `None` only when the document is
/// not one the server knows (not open, never analysed); callers answer
/// `null` for that. An open file with no problems answers an empty full
/// report, not `null` — the empty report is the protocol's "clean" answer.
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
    drop(recorded);

    let full = match kind {
        DocumentDiagnosticReportKind::Full(full) => full,
        // The policy never answers `unchanged` (pinned by the report
        // tests); this arm exists only for exhaustiveness and degrades to
        // an empty report rather than a panic.
        DocumentDiagnosticReportKind::Unchanged(_) => FullDocumentDiagnosticReport::default(),
    };
    tracing::debug!(items = full.items.len(), "pull diagnostics answered");
    Some(DocumentDiagnosticReportResult::Report(
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: full,
        }),
    ))
}
