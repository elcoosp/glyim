//! File create/delete notification support (feature tag
//! LSP-FILE-CREATE-DELETE).
//!
//! The watched-files seam (LSP-WATCHED-FILES) refreshes what the
//! client's *watchers* report; `workspace/didCreateFiles` and
//! `workspace/didDeleteFiles` are the client's own *declaration* of the
//! operations its workspace UI performed (a file explorer's new-file
//! and delete actions, §Workspace: File Operations). Neither endpoint
//! had a route: a file created through the editor's explorer stayed
//! invisible to bare-usage resolution until someone opened it, and a
//! file deleted through the explorer kept its stale facts until its
//! watcher fired. This module closes that capability gap over the same
//! policy the watched seam runs — deliberately: the two notifications
//! are the declared counterpart of the forwarded events, and the same
//! ownership rules decide. A created file no editor owns re-analyses
//! from disk (its definitions may resolve bare usages in the open
//! documents, and the driver's dependent refresh follows); a deleted
//! file no editor owns is forgotten (the session's last facts about a
//! path that no longer exists must not survive the deletion); and a
//! path the editor owns is skipped on both shapes — the client that
//! performed the operation drives the buffer's lifecycle
//! (didOpen/didClose), and a disk read would race the editor's buffer.
//!
//! The seam: the *policy* ([`crate::watched_files::
//! compute_watched_changes`]) and its source trait are the watched
//! seam's own — reusing them keeps one ownership rule per question, so
//! a future registration change cannot make the declared and the
//! forwarded shapes disagree. This module owns only what is genuinely
//! new: the wire mapping ([`created_file_changes`],
//! [`deleted_file_changes`]) and the entry points over the session's
//! own state ([`created_file_decisions`], [`deleted_file_decisions`]).
//!
//! Wire mapping (LSP spec): the params' `FileCreate`/`FileDelete`
//! entries become the wire-free [`WatchedChange`] facts (URI →
//! filesystem path, the declared kind) the policy consumes, with a
//! non-`file:` URI falling back to its path component — the same
//! mapping the document-sync notifications use. The capability is
//! advertised as `workspace.fileOperations.didCreate`/`didDelete` with
//! a `**/*.g` filter, and the conformance test pins the advertisement
//! to the route registrations.

use crate::AnalysisDatabase;
use crate::watched_files::{WatchedChange, WatchedChangeKind};
use lsp_types::{CreateFilesParams, DeleteFilesParams};
use url::Url;

/// Map one URI to the filesystem path the policy consumes (non-`file:`
/// URIs fall back to their path component — the same mapping the
/// document-sync notifications use).
fn path_from_uri(uri: &str) -> std::path::PathBuf {
    Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .unwrap_or_else(|| std::path::PathBuf::from(uri))
}

/// Map the wire creates to the wire-free facts the watched-files policy
/// consumes: URI → filesystem path, `Created` kind.
#[must_use]
pub fn created_file_changes(params: &CreateFilesParams) -> Vec<WatchedChange> {
    params
        .files
        .iter()
        .map(|file| WatchedChange {
            path: path_from_uri(file.uri.as_str()),
            kind: WatchedChangeKind::Created,
        })
        .collect()
}

/// Map the wire deletes to the wire-free facts the watched-files policy
/// consumes: URI → filesystem path, `Deleted` kind.
#[must_use]
pub fn deleted_file_changes(params: &DeleteFilesParams) -> Vec<WatchedChange> {
    params
        .files
        .iter()
        .map(|file| WatchedChange {
            path: path_from_uri(file.uri.as_str()),
            kind: WatchedChangeKind::Deleted,
        })
        .collect()
}

/// `workspace/didCreateFiles` entry point (LSP-FILE-CREATE-DELETE).
///
/// The wire creates are mapped to facts and run through the watched
/// policy over the session's own state; the caller executes the
/// decisions exactly like the watched route (each re-analysis reads the
/// file from disk and feeds the driver, each forget reuses the close
/// cleanup). Pure session-state reads — no disk I/O happens here, so
/// the decision stays answerable on any thread without holding locks
/// across I/O.
#[must_use]
pub fn created_file_decisions(
    db: &AnalysisDatabase,
    params: &CreateFilesParams,
) -> crate::watched_files::WatchedDecisions {
    let changes = created_file_changes(params);
    let file_map = db.file_map.read();
    let watched = db.watched_files.read();
    let source = crate::watched_files::SessionWatchedSource::new(&file_map, &watched);
    crate::watched_files::compute_watched_changes(&changes, &source)
}

/// `workspace/didDeleteFiles` entry point (LSP-FILE-CREATE-DELETE).
///
/// Same shape as [`created_file_decisions`]: the declared deletes run
/// through the watched policy over the session's own state, and the
/// caller executes the decisions.
#[must_use]
pub fn deleted_file_decisions(
    db: &AnalysisDatabase,
    params: &DeleteFilesParams,
) -> crate::watched_files::WatchedDecisions {
    let changes = deleted_file_changes(params);
    let file_map = db.file_map.read();
    let watched = db.watched_files.read();
    let source = crate::watched_files::SessionWatchedSource::new(&file_map, &watched);
    crate::watched_files::compute_watched_changes(&changes, &source)
}
