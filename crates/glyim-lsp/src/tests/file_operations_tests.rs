//! Database-backed tests for the file create/delete notifications
//! (LSP-FILE-CREATE-DELETE).
//!
//! Mirrors `watched_files_tests.rs` and `file_rename_tests.rs`: the
//! session state populates the database exactly like the analysis
//! driver keeps it, and the public `created_file_decisions` /
//! `deleted_file_decisions` entry points are exercised end-to-end (URI
//! → filesystem paths → the ownership policy over the session's own
//! state). The decisions reuse the watched-files policy on purpose —
//! the file-operation notifications are the client's *declaration* of
//! an operation it performed, and the same ownership rules apply — so
//! the pins here cover the wire shape, the entry points, and the
//! advertisement pairing, not the policy internals (owned by
//! `watched_files.rs`).

use crate::AnalysisDatabase;
use crate::file_operations::{created_file_decisions, deleted_file_decisions};
use crate::watched_files::WatchedChangeKind;
use lsp_types::notification::{DidCreateFiles, DidDeleteFiles, Notification};
use lsp_types::{CreateFilesParams, DeleteFilesParams, FileCreate, FileDelete};
use std::path::PathBuf;
use url::Url;

fn uri_str(path: &str) -> String {
    Url::from_file_path(path).unwrap().to_string()
}

/// The wire contracts the engine implements: the notification METHOD
/// strings are pinned so an lsp-types upgrade that reshapes the
/// endpoints surfaces here, not on a client's screen.
#[test]
fn wire_methods_are_pinned() {
    assert_eq!(DidCreateFiles::METHOD, "workspace/didCreateFiles");
    assert_eq!(DidDeleteFiles::METHOD, "workspace/didDeleteFiles");
}

/// The created files map to the wire-free facts the watched-files
/// policy consumes: URI → filesystem path, `Created` kind. A non-`file:`
/// URI falls back to its path component — the same mapping the
/// document-sync notifications use.
#[test]
fn the_created_files_map_to_facts() {
    let params = CreateFilesParams {
        files: vec![FileCreate {
            uri: uri_str("/test/new.g"),
        }],
    };
    let facts = crate::file_operations::created_file_changes(&params);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].path, PathBuf::from("/test/new.g"));
    assert_eq!(facts[0].kind, WatchedChangeKind::Created);
}

/// The deleted files map the same way, with the `Deleted` kind.
#[test]
fn the_deleted_files_map_to_facts() {
    let params = DeleteFilesParams {
        files: vec![FileDelete {
            uri: uri_str("/test/gone.g"),
        }],
    };
    let facts = crate::file_operations::deleted_file_changes(&params);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].path, PathBuf::from("/test/gone.g"));
    assert_eq!(facts[0].kind, WatchedChangeKind::Deleted);
}

/// A client-created file no editor owns and the session never knew
/// re-analyses from disk: its definitions may resolve bare usages in
/// the open documents, exactly like a watcher-reported create.
#[test]
fn a_client_created_unknown_file_reanalyses() {
    let db = AnalysisDatabase::new();
    let params = CreateFilesParams {
        files: vec![FileCreate {
            uri: uri_str("/test/new.g"),
        }],
    };
    let decisions = created_file_decisions(&db, &params);
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/test/new.g")]);
    assert!(decisions.forget.is_empty());
}

/// A client-deleted watched load is forgotten: the session's last facts
/// about a path that no longer exists must not survive the deletion —
/// the watched-files seam's own rule, applied to the declared shape.
#[test]
fn a_client_deleted_watched_file_is_forgotten() {
    let db = AnalysisDatabase::new();
    let path = PathBuf::from("/test/watched.g");
    db.file_map.write().get_or_create(&path);
    db.watched_files.write().insert(path.clone());
    let params = DeleteFilesParams {
        files: vec![FileDelete {
            uri: uri_str("/test/watched.g"),
        }],
    };
    let decisions = deleted_file_decisions(&db, &params);
    assert_eq!(decisions.forget, vec![path]);
    assert!(decisions.reanalyse.is_empty());
}

/// The editor's own documents are skipped on both shapes: the client
/// that performed the operation drives the buffer's lifecycle
/// (didOpen/didClose), and a disk read would race the editor.
#[test]
fn the_editor_owned_documents_are_skipped() {
    let db = AnalysisDatabase::new();
    let open = PathBuf::from("/test/open.g");
    db.file_map.write().get_or_create(&open);
    let created = created_file_decisions(
        &db,
        &CreateFilesParams {
            files: vec![FileCreate {
                uri: uri_str("/test/open.g"),
            }],
        },
    );
    assert!(
        created.reanalyse.is_empty() && created.forget.is_empty(),
        "didCreateFiles for an editor-owned path decides nothing"
    );
    let deleted = deleted_file_decisions(
        &db,
        &DeleteFilesParams {
            files: vec![FileDelete {
                uri: uri_str("/test/open.g"),
            }],
        },
    );
    assert!(
        deleted.reanalyse.is_empty() && deleted.forget.is_empty(),
        "didDeleteFiles for an editor-owned path decides nothing"
    );
}
