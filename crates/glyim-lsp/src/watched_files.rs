//! Watched-files notification support (feature tag LSP-WATCHED-FILES).
//!
//! The session's cross-file facts were only ever refreshed through the
//! editor's own notifications: a file changed by an external tool (a
//! code generator, a format-on-save write, a branch switch) stayed
//! stale until someone opened it, and `workspace/didChangeWatchedFiles`
//! — the notification LSP clients forward for exactly this — had no
//! route. Every input the refresh needs was already recorded: the file
//! map holds the session's known documents, the dependency graph holds
//! the per-file edges the driver's dependent refresh walks, and the
//! watched-path set records what the server loaded itself. This module
//! closes that capability gap: watched events decide, per file, whether
//! the session re-analyses it from disk (a non-open file whose content
//! moved on) or forgets it (a non-open file that no longer exists) —
//! and the driver's existing coalescing and dependent-refresh machinery
//! carries the refresh through, with no new driver-side analysis code.
//!
//! The seam: *which paths the editor owns* is split from *which events
//! the session acts on* by [`WatchedSessionSource`], mirroring
//! LSP-WILL-SAVE's `SaveFormatSource`. The policy
//! ([`compute_watched_changes`]) reads through the trait and stays
//! testable without a database or lock (`tests/watched_files.rs`); the
//! production [`SessionWatchedSource`] reads the driver's file map and
//! the watched-path set — a future source over an explicit client
//! registration would slot in behind the same seam without touching
//! the policy.
//!
//! What the policy decides, per event: a Created/Changed file the
//! editor does not own re-analyses from disk (its definitions may
//! resolve bare usages in the open documents, and the driver's
//! dependent refresh follows from the re-analysis); a Deleted file the
//! editor does not own is forgotten (the session's last facts about a
//! path that no longer exists must not survive the deletion); a file
//! the editor *does* own is skipped on both paths — the client's
//! didChange/didClose drive its content and lifecycle, and a disk read
//! would race the editor's buffer. Event order is preserved and
//! duplicate paths collapse, so one watched-event burst answers one
//! composed decision.
//!
//! Wire mapping (LSP spec): the params' `FileEvent`s become the
//! wire-free [`WatchedChange`] facts (URI → filesystem path, change
//! type → kind) the policy consumes, and the route executes the
//! decisions: each re-analysis reads the file from disk and feeds the
//! driver's `WatchedFileChanged` message (a read failure is a logged
//! skip, never a panic — one unreadable file must not erase the good
//! decisions); each forget reuses the driver's `FileClosed` cleanup,
//! which clears the recorded facts and publishes an empty diagnostics
//! report for the path. lsp-types 0.97 carries no server capability
//! field for this notification (the spec gates it on client-side
//! registration, not on a static capability), so the router
//! registration *is* the advertisement — pinned by the wire and
//! conformance tests.

use crate::AnalysisDatabase;
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use url::Url;

/// The change kinds the policy distinguishes (LSP spec's
/// `FileChangeType`, mapped losslessly; a kind lsp-types does not know
/// maps to [`WatchedChangeKind::Changed`] — the conservative "content
/// may have moved" case).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchedChangeKind {
    /// `FileChangeType::CREATED` — the file appeared.
    Created,
    /// `FileChangeType::CHANGED` — the file's content moved on.
    Changed,
    /// `FileChangeType::DELETED` — the file is gone.
    Deleted,
}

/// One watched-file event, wire-type-free: the filesystem path the event
/// names and the kind of change it reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedChange {
    /// The event's filesystem path.
    pub path: PathBuf,
    /// The reported change kind.
    pub kind: WatchedChangeKind,
}

/// The decisions one watched-event burst answers with: the paths to
/// re-analyse from disk (each becomes one driver message) and the paths
/// to forget (each becomes the driver's close cleanup), both in event
/// order, duplicates collapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchedDecisions {
    /// Non-open paths whose disk content the session must re-analyse.
    pub reanalyse: Vec<PathBuf>,
    /// Non-open paths the session must forget (deleted on disk).
    pub forget: Vec<PathBuf>,
}

/// The watched-files session seam (LSP-WATCHED-FILES).
///
/// The policy in [`compute_watched_changes`] must stay testable without
/// a live `AnalysisDatabase` (locks, session state), so it asks "who
/// owns this path" through this trait. The production
/// [`SessionWatchedSource`] reads the driver's file map and the
/// watched-path set; a future source over an explicit client
/// registration would slot in behind the same seam without touching the
/// policy.
pub trait WatchedSessionSource {
    /// True when the *editor* owns the document's content
    /// (didOpen/didChange drive it): a watched event for it is
    /// redundant, and its lifecycle (didClose) belongs to the client. A
    /// path the server loaded through watching is session-known but not
    /// editor-owned, so a later watched event re-analyses it.
    fn editor_owned(&self, path: &Path) -> bool;
}

/// Production [`WatchedSessionSource`]: derives editor ownership from
/// the session state — a path is editor-owned when the file map knows
/// it and the watched-path set does not claim it.
pub struct SessionWatchedSource<'a> {
    file_map: &'a crate::database::FileMap,
    watched: &'a HashSet<PathBuf>,
}

impl<'a> SessionWatchedSource<'a> {
    /// Bind the source to one session's file map and watched set.
    #[must_use]
    pub fn new(file_map: &'a crate::database::FileMap, watched: &'a HashSet<PathBuf>) -> Self {
        Self { file_map, watched }
    }
}

impl WatchedSessionSource for SessionWatchedSource<'_> {
    fn editor_owned(&self, path: &Path) -> bool {
        self.file_map.get_by_path(path).is_some() && !self.watched.contains(path)
    }
}

/// The watched-files policy: per event, the decision the session acts
/// on, in event order, duplicate paths collapsed.
///
/// Created/Changed files the editor does not own re-analyse from disk;
/// Deleted files the editor does not own are forgotten; editor-owned
/// paths are skipped on both paths (see the module docs for the
/// ownership rules).
#[must_use]
pub fn compute_watched_changes(
    changes: &[WatchedChange],
    source: &dyn WatchedSessionSource,
) -> WatchedDecisions {
    let mut decisions = WatchedDecisions::default();
    for change in changes {
        if change.path.as_os_str().is_empty() {
            // A path-less event names no file and decides nothing.
            continue;
        }
        match change.kind {
            WatchedChangeKind::Created | WatchedChangeKind::Changed => {
                if source.editor_owned(&change.path) {
                    // The client's didChange drives this content — the
                    // watched event is redundant by definition.
                    continue;
                }
                if !decisions.reanalyse.contains(&change.path) {
                    decisions.reanalyse.push(change.path.clone());
                }
            }
            WatchedChangeKind::Deleted => {
                if source.editor_owned(&change.path) {
                    // The client's didClose owns this lifecycle.
                    continue;
                }
                if !decisions.forget.contains(&change.path) {
                    decisions.forget.push(change.path.clone());
                }
            }
        }
    }
    decisions
}

/// Map the wire events to the wire-free facts the policy consumes: URI →
/// filesystem path (non-`file:` URIs fall back to their path component,
/// the same mapping the document-sync notifications use), and the
/// lsp-types change type → the engine's kind, with unknown kinds mapped
/// to `Changed` (the conservative "content may have moved" case).
#[must_use]
pub fn watched_file_changes(params: &DidChangeWatchedFilesParams) -> Vec<WatchedChange> {
    params
        .changes
        .iter()
        .map(|event| {
            let path = Url::parse(event.uri.as_str())
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .unwrap_or_else(|| PathBuf::from(event.uri.as_str()));
            let kind = if event.typ == FileChangeType::CREATED {
                WatchedChangeKind::Created
            } else if event.typ == FileChangeType::DELETED {
                WatchedChangeKind::Deleted
            } else {
                WatchedChangeKind::Changed
            };
            WatchedChange { path, kind }
        })
        .collect()
}

/// `workspace/didChangeWatchedFiles` entry point (LSP-WATCHED-FILES).
///
/// The wire events are mapped to facts and run through the policy over
/// the session's own state; the caller executes the decisions (each
/// re-analysis reads the file from disk and feeds the driver, each
/// forget reuses the driver's close cleanup). Pure session-state reads —
/// no disk I/O happens here, so the decision stays answerable on any
/// thread without holding locks across I/O.
#[must_use]
pub fn watched_file_decisions(
    db: &AnalysisDatabase,
    params: &DidChangeWatchedFilesParams,
) -> WatchedDecisions {
    let changes = watched_file_changes(params);
    let file_map = db.file_map.read();
    let watched = db.watched_files.read();
    let source = SessionWatchedSource::new(&file_map, &watched);
    compute_watched_changes(&changes, &source)
}
