//! Database-backed tests for the watched-files notification
//! (LSP-WATCHED-FILES).
//!
//! Mirrors `inline_value_tests.rs`: the session database is populated
//! exactly like the driver keeps it (file map, dependency graph, the
//! watched-path set), and the public `watched_file_decisions` entry
//! point is exercised end-to-end (wire `FileEvent`s → session state →
//! the reanalyse/forget decisions the router executes). The engine
//! reads session state only — no parse, HIR, or typeck pass is on its
//! path, so the fixtures populate exactly what the engine reads.

use crate::AnalysisDatabase;
use crate::watched_files::{WatchedChangeKind, watched_file_changes, watched_file_decisions};
use lsp_types::notification::DidChangeWatchedFiles as DidChangeWatchedFilesNotification;
use lsp_types::notification::Notification;
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &str) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// Open one document in the session (the driver's file-map state after a
/// didOpen), optionally record it as watched, and register a dependency
/// edge `dependent -> dep` (the driver's `extract_dependencies` state).
fn populate_session(
    db: &AnalysisDatabase,
    open_path: &str,
    dep_path: Option<&str>,
    watched_paths: &[&str],
) {
    db.file_map.write().get_or_create(&PathBuf::from(open_path));
    {
        let mut watched = db.watched_files.write();
        for path in watched_paths {
            watched.insert(PathBuf::from(path));
        }
    }
    if let Some(dep) = dep_path {
        db.dep_graph
            .write()
            .add_dep(PathBuf::from(open_path), PathBuf::from(dep));
    }
}

fn params(events: Vec<FileEvent>) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams { changes: events }
}

/// A changed non-open file the server knows through watching re-analyses
/// — the recorded watched set marks it session-known but not
/// editor-owned.
#[test]
fn a_watched_file_change_reanalyses() {
    let db = AnalysisDatabase::new();
    populate_session(&db, "/ws/open.g", None, &["/ws/generated.g"]);

    let params = params(vec![FileEvent::new(
        uri_for("/ws/generated.g"),
        FileChangeType::CHANGED,
    )]);
    let decisions = watched_file_decisions(&db, &params);
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/ws/generated.g")]);
    assert!(decisions.forget.is_empty());
}

/// A changed editor-open file is skipped — the client's didChange owns
/// its content, so the watched event is redundant.
#[test]
fn an_editor_open_file_change_is_skipped() {
    let db = AnalysisDatabase::new();
    populate_session(&db, "/ws/open.g", None, &[]);

    let params = params(vec![FileEvent::new(
        uri_for("/ws/open.g"),
        FileChangeType::CHANGED,
    )]);
    let decisions = watched_file_decisions(&db, &params);
    assert!(decisions.reanalyse.is_empty());
    assert!(decisions.forget.is_empty());
}

/// A deleted watched file is forgotten — the router clears the session's
/// last facts about a path that no longer exists.
#[test]
fn a_watched_file_deletion_is_forgotten() {
    let db = AnalysisDatabase::new();
    populate_session(&db, "/ws/open.g", None, &["/ws/generated.g"]);

    let params = params(vec![FileEvent::new(
        uri_for("/ws/generated.g"),
        FileChangeType::DELETED,
    )]);
    let decisions = watched_file_decisions(&db, &params);
    assert!(decisions.reanalyse.is_empty());
    assert_eq!(decisions.forget, vec![PathBuf::from("/ws/generated.g")]);
}

/// A created non-open file re-analyses — the new file's definitions may
/// resolve bare usages in the open documents the driver refreshes.
#[test]
fn a_created_file_reanalyses() {
    let db = AnalysisDatabase::new();
    populate_session(&db, "/ws/open.g", None, &[]);

    let params = params(vec![FileEvent::new(
        uri_for("/ws/new.g"),
        FileChangeType::CREATED,
    )]);
    let decisions = watched_file_decisions(&db, &params);
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/ws/new.g")]);
    assert!(decisions.forget.is_empty());
}

/// The wire contract the engine implements: the lsp-types notification
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    assert_eq!(
        DidChangeWatchedFilesNotification::METHOD,
        "workspace/didChangeWatchedFiles"
    );
}

/// The wire event conversion: `FileEvent`s become the wire-free facts
/// the policy consumes — URI → filesystem path, and the lsp-types change
/// types → the engine's kinds.
#[test]
fn wire_events_map_to_facts() {
    let params = params(vec![
        FileEvent::new(uri_for("/ws/created.g"), FileChangeType::CREATED),
        FileEvent::new(uri_for("/ws/changed.g"), FileChangeType::CHANGED),
        FileEvent::new(uri_for("/ws/deleted.g"), FileChangeType::DELETED),
    ]);
    let facts = watched_file_changes(&params);
    let kinds: Vec<(String, WatchedChangeKind)> = facts
        .into_iter()
        .map(|fact| (fact.path.display().to_string(), fact.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("/ws/created.g".to_string(), WatchedChangeKind::Created),
            ("/ws/changed.g".to_string(), WatchedChangeKind::Changed),
            ("/ws/deleted.g".to_string(), WatchedChangeKind::Deleted),
        ]
    );
}
