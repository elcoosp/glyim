//! File-rename session support (feature tag LSP-FILE-RENAME).
//!
//! The client drives the editor's documents, but a file move on disk
//! invalidates the session's *path-keyed* state in ways neither sync
//! notification describes: didChange carries content, didClose carries
//! a lifecycle end, and a renamed document is neither — its facts are
//! still true, only the path they are keyed by moved. `workspace/
//! didRenameFiles` is the notification LSP clients forward for exactly
//! this (§Workspace: File Operations), and it had no route: after a
//! rename, goto-definition and find-references still landed on the old
//! path, and the dependency graph's edges pointed at a file that no
//! longer existed. This module closes that capability gap: every
//! rename is decided *per session role* — the editor-owned documents
//! re-point in place (the recorded facts are keyed by the `FileId`, so
//! preserving the id keeps symbol index, reference graph, HIR, parse
//! trees, typeck state, and recorded diagnostics valid), and the
//! session-known paths the server loaded itself through watching
//! (LSP-WATCHED-FILES) follow the watched-files honest refresh instead:
//! forget the old path, re-analyse the new one from disk, because the
//! disk content moved with the file and the session does not own the
//! buffer.
//!
//! The seam: *which paths the session knows* and *which of them the
//! editor owns* is split from *which actions the rename answers with*
//! by [`RenameSessionSource`], mirroring LSP-WATCHED-FILES's
//! `WatchedSessionSource`. The policy ([`compute_rename_decisions`])
//! reads through the trait and stays testable without a database, file
//! map, or lock (`tests/file_rename.rs`); the production
//! [`SessionRenameSource`] reads the driver's file map and the
//! watched-path set — a future source over an explicit client
//! registration would slot in behind the same seam without touching
//! the policy.
//!
//! What the policy decides, per rename event, in event order with
//! duplicate old paths collapsed to the first decision: a rename whose
//! old path the session never knew answers nothing (clients report
//! workspace-wide operations; the server has no facts to move for a
//! file it does not know); a rename onto a path the session already
//! knows collides — re-pointing would orphan the existing path's facts,
//! so it is a skip too, never a half-applied rename; an editor-owned
//! rename answers a *re-key* (the driver moves the path-keyed state and
//! the facts survive); a watched rename answers a *refresh* (forget
//! old, re-analyse new from disk — the watched seam's own semantics).
//!
//! Wire mapping (LSP spec): the params' `FileRename` pairs become the
//! wire-free [`FileRename`] facts (URI → filesystem path, old and new
//! kept paired) the policy consumes, with a non-`file:` URI falling
//! back to its path component — the same mapping the document-sync
//! notifications use. The capability is advertised as
//! `workspace.fileOperations.didRename` with a `**/*.g` filter, and the
//! conformance test pins the advertisement to the route registration.

use crate::AnalysisDatabase;
use lsp_types::RenameFilesParams;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use url::Url;

/// One client-reported file rename, wire-type-free: the old filesystem
/// path the file moved from and the new one it moved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRename {
    /// The path the file moved from.
    pub old: PathBuf,
    /// The path the file moved to.
    pub new: PathBuf,
}

/// The decisions one rename burst answers with: the in-place re-keys
/// (each becomes one driver message that re-points the session's
/// path-keyed state) and the disk refreshes (each forgets the old path
/// and re-analyses the new one), both in event order, old-path
/// duplicates collapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenamedDecisions {
    /// Editor-owned renames: re-point the session's path-keyed state in
    /// place — the `FileId` and every fact keyed by it survive.
    pub rekey: Vec<FileRename>,
    /// Watched renames: forget the old path and re-analyse the new one
    /// from disk (the watched-files seam's own refresh semantics).
    pub refresh: Vec<FileRename>,
}

/// The file-rename session seam (LSP-FILE-RENAME).
///
/// The policy in [`compute_rename_decisions`] must stay testable without
/// a live `AnalysisDatabase` (locks, session state), so it asks "does
/// the session know this path, and who owns it" through this trait. The
/// production [`SessionRenameSource`] reads the driver's file map and
/// the watched-path set; a future source over an explicit client
/// registration would slot in behind the same seam without touching the
/// policy.
pub trait RenameSessionSource {
    /// True when the session holds recorded facts for the path (the
    /// file map knows it): only such paths carry facts a rename can
    /// move.
    fn session_known(&self, path: &Path) -> bool;

    /// True when the *editor* owns the document's content
    /// (didOpen/didChange drive it): its rename re-points the session
    /// in place, preserving the facts. A session-known path the editor
    /// does *not* own is the server's own watched load — its rename is
    /// a disk refresh instead.
    fn editor_owned(&self, path: &Path) -> bool;
}

/// Production [`RenameSessionSource`]: derives session knowledge and
/// editor ownership from the session state — a path is session-known
/// when the file map knows it, and editor-owned when the watched-path
/// set does not claim it.
pub struct SessionRenameSource<'a> {
    file_map: &'a crate::database::FileMap,
    watched: &'a HashSet<PathBuf>,
}

impl<'a> SessionRenameSource<'a> {
    /// Bind the source to one session's file map and watched set.
    #[must_use]
    pub fn new(file_map: &'a crate::database::FileMap, watched: &'a HashSet<PathBuf>) -> Self {
        Self { file_map, watched }
    }
}

impl RenameSessionSource for SessionRenameSource<'_> {
    fn session_known(&self, path: &Path) -> bool {
        self.file_map.get_by_path(path).is_some()
    }

    fn editor_owned(&self, path: &Path) -> bool {
        self.file_map.get_by_path(path).is_some() && !self.watched.contains(path)
    }
}

/// The file-rename policy: per event, the decision the session acts on,
/// in event order, duplicate old paths collapsed to the first decision.
///
/// Editor-owned renames re-key in place; watched renames refresh (forget
/// old, re-analyse new from disk); renames of unknown old paths or onto
/// known new paths answer nothing (see the module docs for the
/// ownership and collision rules).
#[must_use]
pub fn compute_rename_decisions(
    renames: &[FileRename],
    source: &dyn RenameSessionSource,
) -> RenamedDecisions {
    let mut decisions = RenamedDecisions::default();
    let mut decided_olds: Vec<PathBuf> = Vec::new();
    for rename in renames {
        if rename.old.as_os_str().is_empty() || rename.new.as_os_str().is_empty() {
            // A path-less rename names no file and decides nothing.
            continue;
        }
        if decided_olds.contains(&rename.old) {
            // One rename burst answers at most one action per old path.
            continue;
        }
        if !source.session_known(&rename.old) || source.session_known(&rename.new) {
            // An unknown old path carries no facts to move; a known new
            // path collides — re-pointing would orphan its facts. Both
            // answer nothing.
            decided_olds.push(rename.old.clone());
            continue;
        }
        if source.editor_owned(&rename.old) {
            decisions.rekey.push(rename.clone());
        } else {
            decisions.refresh.push(rename.clone());
        }
        decided_olds.push(rename.old.clone());
    }
    decisions
}

/// Map the wire renames to the wire-free facts the policy consumes: URI
/// → filesystem path (non-`file:` URIs fall back to their path
/// component, the same mapping the document-sync notifications use),
/// old and new kept paired.
#[must_use]
pub fn renamed_files(params: &RenameFilesParams) -> Vec<FileRename> {
    params
        .files
        .iter()
        .map(|event| FileRename {
            old: Url::parse(event.old_uri.as_str())
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .unwrap_or_else(|| PathBuf::from(event.old_uri.as_str())),
            new: Url::parse(event.new_uri.as_str())
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .unwrap_or_else(|| PathBuf::from(event.new_uri.as_str())),
        })
        .collect()
}

/// `workspace/didRenameFiles` entry point (LSP-FILE-RENAME).
///
/// The wire renames are mapped to facts and run through the policy over
/// the session's own state; the caller executes the decisions (each
/// re-key becomes the driver's `FileRenamed` message; each refresh
/// forgets the old path and re-analyses the new one from disk — the
/// watched route's own execution). Pure session-state reads — no disk
/// I/O happens here, so the decision stays answerable on any thread
/// without holding locks across I/O.
#[must_use]
pub fn renamed_file_decisions(
    db: &AnalysisDatabase,
    params: &RenameFilesParams,
) -> RenamedDecisions {
    let renames = renamed_files(params);
    let file_map = db.file_map.read();
    let watched = db.watched_files.read();
    let source = SessionRenameSource::new(&file_map, &watched);
    compute_rename_decisions(&renames, &source)
}
