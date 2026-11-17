//! Policy-level tests for the watched-files engine (LSP-WATCHED-FILES).
//!
//! Mirrors `inline_value.rs` and `will_save.rs`: the *policy* (which
//! watched file events the session acts on) is pinned through an
//! in-memory [`MockSession`] implementing
//! [`crate::watched_files::WatchedSessionSource`], decoupled from any
//! database, lock, or disk read. The database-backed production path is
//! covered end-to-end in `watched_files_tests.rs`.

use crate::watched_files::{
    WatchedChange, WatchedChangeKind, WatchedSessionSource, compute_watched_changes,
};
use std::path::Path;
use std::path::PathBuf;

/// In-memory session state: the policy-side test double. `open` models
/// the documents the editor owns; `watched` models the paths the server
/// loaded through watching — the two sets the production source derives
/// from the database.
#[derive(Default)]
struct MockSession {
    open: Vec<PathBuf>,
    watched: Vec<PathBuf>,
}

impl WatchedSessionSource for MockSession {
    fn editor_owned(&self, path: &Path) -> bool {
        self.open.iter().any(|p| p == path) && !self.watched.iter().any(|p| p == path)
    }
}

impl MockSession {
    fn opened(&mut self, path: &str) {
        self.open.push(PathBuf::from(path));
    }

    fn watched_path(&mut self, path: &str) {
        self.watched.push(PathBuf::from(path));
    }
}

fn change(kind: WatchedChangeKind, path: &str) -> WatchedChange {
    WatchedChange {
        path: PathBuf::from(path),
        kind,
    }
}

/// A changed non-open file re-analyses from disk: the session's cached
/// facts about it (and everything that depends on it, via the driver's
/// own dependent refresh) must follow the disk content, not the stale
/// in-memory one.
#[test]
fn a_changed_non_open_file_reanalyses() {
    let mut session = MockSession::default();
    session.opened("/ws/open.g");
    let decisions = compute_watched_changes(
        &[change(WatchedChangeKind::Changed, "/ws/on_disk.g")],
        &session,
    );
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/ws/on_disk.g")]);
    assert!(decisions.forget.is_empty());
}

/// A changed file the editor owns is skipped: the client's didChange
/// drives its content, and a disk read would race the editor's buffer —
/// the watched event is redundant by definition.
#[test]
fn a_changed_editor_open_file_is_skipped() {
    let mut session = MockSession::default();
    session.opened("/ws/open.g");
    let decisions = compute_watched_changes(
        &[change(WatchedChangeKind::Changed, "/ws/open.g")],
        &session,
    );
    assert!(decisions.reanalyse.is_empty());
    assert!(decisions.forget.is_empty());
}

/// A file the server itself loaded through watching (recorded as
/// watched) is not editor-owned: a later watched event re-analyses it
/// again — external tools may touch it repeatedly, and each event says
/// the disk moved on.
#[test]
fn a_re_watched_file_stays_reanalysable() {
    let mut session = MockSession::default();
    session.watched_path("/ws/generated.g");
    let decisions = compute_watched_changes(
        &[change(WatchedChangeKind::Changed, "/ws/generated.g")],
        &session,
    );
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/ws/generated.g")]);
}

/// A deleted non-open file is forgotten: the session clears its last
/// facts about a path that no longer exists — stale symbols and dep
/// edges must not survive the deletion.
#[test]
fn a_deleted_non_open_file_is_forgotten() {
    let mut session = MockSession::default();
    session.watched_path("/ws/generated.g");
    let decisions = compute_watched_changes(
        &[change(WatchedChangeKind::Deleted, "/ws/generated.g")],
        &session,
    );
    assert!(decisions.reanalyse.is_empty());
    assert_eq!(decisions.forget, vec![PathBuf::from("/ws/generated.g")]);
}

/// A deleted editor-open file is skipped: the client's didClose owns
/// that lifecycle, and the server answering the deletion itself would
/// race the editor's close path.
#[test]
fn a_deleted_editor_open_file_is_skipped() {
    let mut session = MockSession::default();
    session.opened("/ws/open.g");
    let decisions = compute_watched_changes(
        &[change(WatchedChangeKind::Deleted, "/ws/open.g")],
        &session,
    );
    assert!(decisions.reanalyse.is_empty());
    assert!(decisions.forget.is_empty());
}

/// A created file re-analyses like a changed one: the new file's
/// definitions may resolve bare usages in open files.
#[test]
fn a_created_non_open_file_reanalyses() {
    let session = MockSession::default();
    let decisions =
        compute_watched_changes(&[change(WatchedChangeKind::Created, "/ws/new.g")], &session);
    assert_eq!(decisions.reanalyse, vec![PathBuf::from("/ws/new.g")]);
}

/// Event order is preserved, duplicates collapse, and mixed kinds route
/// to the right decision lists: one burst of watched events answers one
/// composed decision.
#[test]
fn a_burst_composes_into_ordered_deduplicated_decisions() {
    let mut session = MockSession::default();
    session.opened("/ws/open.g");
    session.watched_path("/ws/generated.g");
    let decisions = compute_watched_changes(
        &[
            change(WatchedChangeKind::Changed, "/ws/late.g"),
            change(WatchedChangeKind::Deleted, "/ws/open.g"),
            change(WatchedChangeKind::Changed, "/ws/generated.g"),
            change(WatchedChangeKind::Changed, "/ws/late.g"),
        ],
        &session,
    );
    assert_eq!(
        decisions.reanalyse,
        vec![
            PathBuf::from("/ws/late.g"),
            PathBuf::from("/ws/generated.g")
        ]
    );
    assert!(decisions.forget.is_empty(), "the open file is skipped");
}

/// An empty event burst answers an empty decision — the honest
/// answerable-empty, never a guess.
#[test]
fn an_empty_burst_answers_no_decisions() {
    let session = MockSession::default();
    let decisions = compute_watched_changes(&[], &session);
    assert!(decisions.reanalyse.is_empty());
    assert!(decisions.forget.is_empty());
}
