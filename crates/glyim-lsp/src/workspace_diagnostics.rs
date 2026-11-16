//! Workspace-diagnostics request support (feature tag LSP-WORKSPACE-DIAG).
//!
//! The per-file pull (`textDocument/diagnostic`, LSP-PULL-DIAG) answers
//! one recorded set. A client that polls the *whole* session — a batch
//! test runner sweeping every open document after a reconnect, a host
//! reconciling state after a server restart, an editor negotiating the
//! 3.17 pull model end to end — had no way to ask for all of them at
//! once: `workspace/diagnostic` did not exist, and the capability said
//! so honestly (`workspace_diagnostics: false` in `DiagnosticOptions`).
//! This module closes that gap: the answer composes one full report per
//! analysed file out of the same recorded sets the push path publishes
//! and the per-file pull answers, so a client mixing all three paths
//! sees identical coordinates and messages everywhere.
//!
//! The seam: *which files a workspace report covers, and what each
//! entry carries* is split from *how the stores are found* by
//! [`AnalysedSetSource`], mirroring LSP-PULL-DIAG's `DiagnosticSource`
//! (and reusing it — the per-entry sets are read through the same
//! trait, so the two pull paths cannot drift). The composition policy
//! ([`workspace_report`]) reads through the trait and stays testable
//! without a database or lock (`tests/workspace_diagnostics.rs`); the
//! production [`RecordedAnalysedSource`] borrows the driver's stores
//! (`source_maps` = analysed files, `versions` = the `didChange`
//! version each file was analysed at, `diagnostics` = the recorded
//! sets). A future computed-on-demand scheme would slot in behind the
//! same seam without touching the policy.
//!
//! Report honesty: the answer is *always* full reports. The params'
//! `previous_result_ids` are deliberately ignored — unchanged reports
//! are only legal from a server that *issues* result ids, and this one
//! never has (same contract as the per-file pull, pinned by
//! `pull_report`'s tests). Likewise every `resultId` stays `None`. A
//! file analysed without a client version reports `version: None` —
//! the spec's "not marked as open" honesty — never a fabricated 0. The
//! workspace pull is always answerable: even a session with nothing
//! analysed answers an empty `items` array (there is no document the
//! params could name wrongly, so the per-file pull's "unknown document
//! → error" case does not exist here).
//!
//! Order: reports are composed in ascending [`FileId`] order (the
//! allocation order of the session). The recorded stores are `HashMap`s
//! whose iteration order shifts run to run; sorting in the policy is
//! what makes a client's repeated polls mutually comparable.
//!
//! Wire mapping (LSP spec): `WorkspaceDiagnosticReportResult::Report`
//! wrapping one `WorkspaceDocumentDiagnosticReport::Full` per analysed
//! file, each carrying the file's recorded set in the driver's order —
//! the same order `publishDiagnostics` carries — in the same
//! `SourceMap` UTF-16 coordinates every other diagnostic path uses.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::pull_diagnostics::{DiagnosticSource, RecordedDiagnosticSource};
use glyim_span::FileId;
use lsp_types::{
    Diagnostic, FullDocumentDiagnosticReport, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDiagnosticReportResult, WorkspaceDocumentDiagnosticReport,
    WorkspaceFullDocumentDiagnosticReport,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// One analysed file as the workspace-report policy sees it (the
/// [`AnalysedSetSource`] record): identity, the client version it was
/// last analysed at, and the recorded diagnostic set in the driver's
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysedFile {
    /// The file's session id.
    pub file_id: FileId,
    /// The on-disk path the file was opened under.
    pub path: PathBuf,
    /// The `didChange` version the driver last analysed the file at;
    /// `None` when the file has no recorded version (never opened by
    /// the client).
    pub version: Option<i64>,
    /// The recorded diagnostic set, in the driver's recording (pipeline)
    /// order — the same order the push path publishes.
    pub diagnostics: Vec<Diagnostic>,
}

/// The analysed-set seam for workspace-diagnostics requests
/// (LSP-WORKSPACE-DIAG).
///
/// The composition policy in [`workspace_report`] must stay testable
/// without a live `AnalysisDatabase` (locks, maps, drivers), so it reads
/// the session's analysed files through this trait. The production
/// [`RecordedAnalysedSource`] borrows the driver's stores; a future
/// computed-on-demand source would slot in behind the same seam without
/// touching the policy.
pub trait AnalysedSetSource {
    /// One entry per analysed file, in any order (the policy imposes the
    /// deterministic ascending-`FileId` order).
    fn analysed_files(&self) -> Vec<AnalysedFile>;
}

/// Production [`AnalysedSetSource`]: borrows the driver's stores —
/// `source_maps` (whose keys *are* the analysed files: the driver
/// inserts one per analysis and removes it on close), `versions`, and
/// `diagnostics` — and translates ids to paths through the session's
/// `FileMap`.
///
/// A file present in `source_maps` but missing from `file_map` is
/// skipped (it is being torn down by the driver; reporting it would
/// answer with a URI no client holds). Borrowing (not cloning the
/// maps) keeps the request allocation-free apart from the returned
/// reports; the read guards live in the caller.
pub struct RecordedAnalysedSource<'a> {
    file_map: &'a FileMap,
    analysed: &'a HashMap<FileId, crate::database::SourceMap>,
    versions: &'a HashMap<FileId, i64>,
    recorded: &'a HashMap<FileId, Vec<Diagnostic>>,
}

impl<'a> RecordedAnalysedSource<'a> {
    /// Borrows the driver's stores as the workspace source for one
    /// request.
    #[must_use]
    pub fn new(
        file_map: &'a FileMap,
        analysed: &'a HashMap<FileId, crate::database::SourceMap>,
        versions: &'a HashMap<FileId, i64>,
        recorded: &'a HashMap<FileId, Vec<Diagnostic>>,
    ) -> Self {
        Self {
            file_map,
            analysed,
            versions,
            recorded,
        }
    }
}

impl AnalysedSetSource for RecordedAnalysedSource<'_> {
    fn analysed_files(&self) -> Vec<AnalysedFile> {
        // The per-entry sets are read through the *same*
        // [`DiagnosticSource`](crate::pull_diagnostics::DiagnosticSource) seam the per-file pull uses — the
        // missing-entry-is-empty contract (the driver's clearing rule)
        // lives there, so the two pull paths cannot drift.
        let sets = RecordedDiagnosticSource::new(self.recorded);
        self.analysed
            .keys()
            .filter_map(|file_id| {
                // The driver tears `file_map` entries down before the
                // analysis artifacts; a file mid-teardown is not
                // reported (its path — and therefore its URI — no
                // longer exists for any client).
                let path = self.file_map.path(*file_id)?;
                let version = self.versions.get(file_id).copied();
                Some(AnalysedFile {
                    file_id: *file_id,
                    path: path.clone(),
                    version,
                    diagnostics: sets.diagnostics_of(*file_id),
                })
            })
            .collect()
    }
}

/// Workspace-diagnostics composition policy: one full report per
/// analysed file, ascending [`FileId`] order.
///
/// Always full: the params' echoed `previous_result_ids` are ignored
/// (an echoed id must not yield an `unchanged` report — the server
/// never issued one, so it cannot vouch for staleness), and no report
/// invents a `resultId`. An analysed file with no recorded problems
/// contributes a full report with zero items ("checked, nothing to
/// report"), exactly like its per-file pull answer.
#[must_use]
pub fn workspace_report(source: &dyn AnalysedSetSource) -> WorkspaceDiagnosticReport {
    let mut files = source.analysed_files();
    // Ascending file id (allocation order): the recorded stores are
    // `HashMap`s whose iteration order shifts run to run, so the policy
    // — not the production source — owns the deterministic order.
    files.sort_by_key(|file| file.file_id.to_raw());
    let items = files
        .into_iter()
        .filter_map(|file| {
            let wire_uri = Url::from_file_path(&file.path)
                .ok()
                .and_then(|uri| lsp_types::Uri::from_str(uri.as_str()).ok())?;
            Some(WorkspaceDocumentDiagnosticReport::Full(
                WorkspaceFullDocumentDiagnosticReport {
                    uri: wire_uri,
                    version: file.version,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: file.diagnostics,
                    },
                },
            ))
        })
        .collect();
    WorkspaceDiagnosticReport { items }
}

/// `workspace/diagnostic` request entry point (LSP-WORKSPACE-DIAG).
///
/// Composes one full report per analysed file out of the recorded
/// stores, through the same [`DiagnosticSource`](crate::pull_diagnostics::DiagnosticSource) seam the per-file pull
/// uses. The answer is always a report — an empty session answers an
/// empty `items` array — and the params are accepted for signature
/// symmetry with the router's other request handlers only: their
/// `previous_result_ids` are deliberately ignored (see the module doc
/// and [`workspace_report`]).
#[must_use]
pub fn workspace_diagnostics(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    _params: &WorkspaceDiagnosticParams,
) -> WorkspaceDiagnosticReportResult {
    let analysed = db.source_maps.read();
    let versions = db.versions.read();
    let recorded = db.diagnostics.read();
    let source = RecordedAnalysedSource::new(file_map, &analysed, &versions, &recorded);
    let report = workspace_report(&source);
    let files = report.items.len();
    drop(recorded);
    drop(versions);
    drop(analysed);
    tracing::debug!(files, "workspace diagnostics answered");
    WorkspaceDiagnosticReportResult::Report(report)
}
