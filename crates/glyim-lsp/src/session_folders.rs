//! Workspace-folder tracking (feature tag LSP-WORKSPACE-FOLDERS).
//!
//! The session never answered one of the client's first questions: *which
//! workspace folders am I serving?* `ServerCapabilities.workspace_folders`
//! was `None`, the `InitializeParams` folder set was ignored (`_params`),
//! and `workspace/didChangeWorkspaceFolders` had no route — a client that
//! adds or removes a folder changed nothing in the session, and no query
//! could answer "does this file sit inside a folder the session serves?"
//! Every fact the capability needs arrives on the wire the session
//! already reads: the initialize params carry `workspace_folders`
//! (falling back to the deprecated `rootUri` when the client sends no
//! folder list), and the change notification carries the added/removed
//! event. This module closes that capability gap: the session records the
//! folder set — order-stable, first-added first — and answers folder
//! queries off that store, no analysis work involved (folders are
//! metadata; nothing is re-analysed, nothing is re-parsed).
//!
//! The seam: *which folders the session records* is split from *how the
//! driver stores them* by [`FolderSetSource`], mirroring LSP-MONIKER's
//! `MonikerSource`. The queries read through the trait and stay testable
//! without a live `AnalysisDatabase` (`tests/session_folders.rs`); the
//! production [`SessionFolderSource`] reads the shared
//! `AnalysisDatabase::workspace_folders` store. The *apply* policy
//! ([`compute_folder_set`]) is a pure function over records — the same
//! policy the route and the tests exercise, no locks on its path.
//!
//! The honest decisions, over the recorded facts: a folder identity is
//! the *spelled path* (the file map keys everything by the path the
//! client spelled, and the folder set must agree with the set it scopes),
//! so a folder whose URI does not resolve to a file path (`ftp://`,
//! `untitled:`) is skipped, never invented — the session cannot serve a
//! remote folder it cannot key; a re-added folder already recorded is
//! idempotent (the first recording wins: its position and name stand); a
//! removed folder the session does not know is skipped (a refusal must
//! not reorder or truncate the surviving set); adds and removals inside
//! one event apply in event order, so `remove(A); add(A)` re-records A at
//! its new position, exactly what the client spelled.
//!
//! Wire mapping (LSP spec): the capability advertises
//! `workspace.workspaceFolders.supported = true` with
//! `change_notifications = true`, so clients send the change
//! notification; the initialize seed consumes
//! `params.workspace_folders` (or `rootUri` as the compatibility
//! fallback) and consumes nothing else — the params' capabilities block
//! is the client's, not the session's. Queries answer records in
//! first-added order; [`folders_containing`] answers the folders whose
//! path contains the queried path — the scoping primitive the per-folder
//! work (diagnostics, symbols) slots into later.

use crate::AnalysisDatabase;
use lsp_types::DidChangeWorkspaceFoldersParams;
use lsp_types::InitializeParams;
use std::path::{Path, PathBuf};
use url::Url;

/// One workspace folder the session serves, wire-type-free: the client's
/// display name, the URI it was spelled with, and the path form the
/// session keys it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRecord {
    /// The folder's display name (the client's spelling).
    pub name: String,
    /// The URI the folder was spelled with, kept verbatim for round-trips.
    pub uri: String,
    /// The path form the session keys the folder by.
    pub path: PathBuf,
}

/// One folder-set change event, wire-type-free: the folders the client
/// added and removed, in event order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderChange {
    /// The folders the client added, in event order.
    pub added: Vec<FolderRecord>,
    /// The folders the client removed, in event order.
    pub removed: Vec<FolderRecord>,
}

/// The folder-set read seam (LSP-WORKSPACE-FOLDERS).
///
/// The queries ([`recorded_folders`], [`folders_containing`]) must stay
/// testable without a live `AnalysisDatabase` (locks, async runtime), so
/// they read the recorded set through this trait. The production
/// [`SessionFolderSource`] reads the shared
/// `AnalysisDatabase::workspace_folders` store; a future source over a
/// persisted workspace manifest would slot in behind the same seam
/// without touching the queries.
pub trait FolderSetSource {
    /// Every folder the session records, in first-added order.
    fn recorded_folders(&self) -> Vec<FolderRecord>;
}

/// Production [`FolderSetSource`]: reads the shared database store.
pub struct SessionFolderSource<'a> {
    db: &'a AnalysisDatabase,
}

impl<'a> SessionFolderSource<'a> {
    /// Bind the source to one session's folder store.
    #[must_use]
    pub fn new(db: &'a AnalysisDatabase) -> Self {
        Self { db }
    }
}

impl FolderSetSource for SessionFolderSource<'_> {
    fn recorded_folders(&self) -> Vec<FolderRecord> {
        let folders = self.db.workspace_folders.read();
        folders.clone()
    }
}

/// Convert one wire folder into the record the session keys, `None` when
/// the URI does not resolve to a file path — the session is path-keyed
/// end-to-end and cannot serve a folder it cannot key, so a remote or
/// `untitled:` scheme is skipped, never invented.
#[must_use]
pub fn folder_record_from_wire(folder: &lsp_types::WorkspaceFolder) -> Option<FolderRecord> {
    let path = Url::parse(folder.uri.as_str()).ok()?.to_file_path().ok()?;
    Some(FolderRecord {
        name: folder.name.clone(),
        uri: folder.uri.as_str().to_string(),
        path,
    })
}

/// Convert one wire change event into the policy's wire-free shape, with
/// unresolvable folders dropped (see [`folder_record_from_wire`]).
#[must_use]
pub fn folder_change_from_wire(params: &DidChangeWorkspaceFoldersParams) -> FolderChange {
    FolderChange {
        added: params
            .event
            .added
            .iter()
            .filter_map(folder_record_from_wire)
            .collect(),
        removed: params
            .event
            .removed
            .iter()
            .filter_map(folder_record_from_wire)
            .collect(),
    }
}

/// The folder-set apply policy: the set that records the change event,
/// starting from `recorded`.
///
/// Rules, in application order: removals drop the first folder whose path
/// matches (an unknown removal is skipped — a refusal must not reorder or
/// truncate the survivors); additions append a folder whose path is not
/// already recorded and leave an already-recorded one untouched (the
/// first recording wins: its position and name stand); within one event,
/// removals and additions apply in event order, so `remove(A); add(A)`
/// re-records A at its new position, exactly what the client spelled. The
/// result keeps the surviving set's first-added order.
#[must_use]
pub fn compute_folder_set(recorded: &[FolderRecord], change: &FolderChange) -> Vec<FolderRecord> {
    let mut set: Vec<FolderRecord> = recorded.to_vec();
    for removed in &change.removed {
        if let Some(position) = set.iter().position(|folder| folder.path == removed.path) {
            set.remove(position);
        }
    }
    for added in &change.added {
        if !set.iter().any(|folder| folder.path == added.path) {
            set.push(added.clone());
        }
    }
    set
}

/// The folder-of-path query policy: the recorded folders whose path
/// contains `path` — the scoping primitive the per-folder work (diagnostics,
/// symbols) slots into later. Answers in first-added order; a query for a
/// folder path itself answers that folder.
#[must_use]
pub fn compute_folders_containing(recorded: &[FolderRecord], path: &Path) -> Vec<FolderRecord> {
    recorded
        .iter()
        .filter(|folder| path.starts_with(&folder.path))
        .cloned()
        .collect()
}

/// Seed the session's folder set from one client's initialize params
/// (LSP-WORKSPACE-FOLDERS).
///
/// The folders arrive in `params.workspace_folders`; a client that sends
/// none falls back to the deprecated `rootUri` (the compatibility shape
/// older clients still spell). Unresolvable URIs are skipped, never
/// invented. Returns the number of folders recorded; the seed runs once
/// (initialize), and the store starts empty, so the count is the set.
pub fn seed_workspace_folders(db: &AnalysisDatabase, params: &InitializeParams) -> usize {
    let mut seeded = Vec::new();
    match params.workspace_folders.as_ref() {
        Some(folders) if !folders.is_empty() => {
            for folder in folders {
                if let Some(record) = folder_record_from_wire(folder) {
                    seeded.push(record);
                } else {
                    tracing::warn!(
                        uri = %folder.uri.as_str(),
                        "workspace folder URI does not resolve to a file path; folder skipped"
                    );
                }
            }
        }
        _ => {
            if let Some(root) = params.root_uri.as_ref()
                && let Ok(path) = Url::parse(root.as_str()).and_then(|url| {
                    url.to_file_path()
                        .map_err(|()| url::ParseError::RelativeUrlWithoutBase)
                })
            {
                seeded.push(FolderRecord {
                    name: "root".to_string(),
                    uri: root.as_str().to_string(),
                    path,
                });
            }
        }
    }
    let count = seeded.len();
    *db.workspace_folders.write() = seeded;
    tracing::debug!(
        folders = count,
        "workspace folder set seeded from initialize"
    );
    count
}

/// Apply one `workspace/didChangeWorkspaceFolders` event to the session's
/// folder store (LSP-WORKSPACE-FOLDERS).
///
/// The route's whole job is the state edit — folders are metadata, so no
/// analysis work enqueues and no diagnostics republish; the next query
/// that scopes by folders reads the new set. Unresolvable folders in the
/// event are skipped with the same honesty as the seed.
pub fn did_change_workspace_folders(
    db: &AnalysisDatabase,
    params: &DidChangeWorkspaceFoldersParams,
) {
    let change = folder_change_from_wire(params);
    let skipped = params.event.added.len() + params.event.removed.len()
        - change.added.len()
        - change.removed.len();
    let next = {
        let folders = db.workspace_folders.read();
        compute_folder_set(&folders, &change)
    };
    tracing::debug!(
        added = change.added.len(),
        removed = change.removed.len(),
        skipped,
        recorded = next.len(),
        "workspace folder set updated"
    );
    *db.workspace_folders.write() = next;
}

/// The session's recorded folder set, in first-added order
/// (LSP-WORKSPACE-FOLDERS).
#[must_use]
pub fn recorded_folders(db: &AnalysisDatabase) -> Vec<FolderRecord> {
    SessionFolderSource::new(db).recorded_folders()
}

/// The recorded folders whose path contains `path`, in first-added order
/// (LSP-WORKSPACE-FOLDERS) — the scoping primitive the per-folder work
/// slots into later.
#[must_use]
pub fn folders_containing(db: &AnalysisDatabase, path: &Path) -> Vec<FolderRecord> {
    let folders = SessionFolderSource::new(db).recorded_folders();
    compute_folders_containing(&folders, path)
}
