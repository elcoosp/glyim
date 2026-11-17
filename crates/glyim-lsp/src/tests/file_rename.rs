//! Policy-level tests for the file-rename engine (LSP-FILE-RENAME).
//!
//! Mirrors `watched_files.rs` and `will_save.rs`: the *policy* (which
//! session action a client-reported file rename turns into) is pinned
//! through an in-memory [`MockSource`] implementing
//! [`crate::file_rename::RenameSessionSource`], decoupled from any
//! database, source map, or lock. The session-state production path is
//! covered end-to-end in `file_rename_tests.rs`.

use crate::file_rename::{FileRename, RenameSessionSource, compute_rename_decisions};
use std::path::{Path, PathBuf};

/// In-memory session view: the policy-side test double. The policy asks
/// only "who owns this path" and owns every decision on top.
struct MockSource {
    known: Vec<PathBuf>,
    owned: Vec<PathBuf>,
}

impl RenameSessionSource for MockSource {
    fn session_known(&self, path: &Path) -> bool {
        self.known.iter().any(|p| p == path)
    }

    fn editor_owned(&self, path: &Path) -> bool {
        self.owned.iter().any(|p| p == path)
    }
}

fn rename(old: &str, new: &str) -> FileRename {
    FileRename {
        old: PathBuf::from(old),
        new: PathBuf::from(new),
    }
}

/// A rename of a document the editor owns (didOpen/didChange drive its
/// content) re-points the session in place: the recorded facts are keyed
/// by the file id, so preserving the id keeps them valid — a re-key, not
/// a forget-and-rebuild.
#[test]
fn an_editor_owned_rename_answers_a_rekey() {
    let source = MockSource {
        known: vec![PathBuf::from("/ws/main.g")],
        owned: vec![PathBuf::from("/ws/main.g")],
    };
    let decisions = compute_rename_decisions(&[rename("/ws/main.g", "/ws/lib.g")], &source);
    assert_eq!(decisions.rekey, vec![rename("/ws/main.g", "/ws/lib.g")]);
    assert!(
        decisions.refresh.is_empty(),
        "no disk re-analysis: the editor drives the content"
    );
}

/// A rename of a file the server loaded itself through watching (not
/// editor-owned) is a forget-and-rebuild: the old path's facts are
/// forgotten and the new path re-analyses from disk — the same honest
/// refresh the watched-files seam runs, because the disk content may
/// have moved with the file.
#[test]
fn a_watched_rename_answers_a_refresh() {
    let source = MockSource {
        known: vec![PathBuf::from("/ws/generated.g")],
        owned: vec![],
    };
    let decisions =
        compute_rename_decisions(&[rename("/ws/generated.g", "/ws/renamed.g")], &source);
    assert_eq!(
        decisions.refresh,
        vec![rename("/ws/generated.g", "/ws/renamed.g")]
    );
    assert!(
        decisions.rekey.is_empty(),
        "no in-place re-key: the session does not own the content"
    );
}

/// A rename whose old path the session has never seen answers nothing:
/// the client reports file operations for the whole workspace, and the
/// server has no facts to move for a file it does not know.
#[test]
fn an_unknown_old_path_answers_nothing() {
    let source = MockSource {
        known: vec![],
        owned: vec![],
    };
    let decisions =
        compute_rename_decisions(&[rename("/ws/stranger.g", "/ws/elsewhere.g")], &source);
    assert!(decisions.rekey.is_empty());
    assert!(decisions.refresh.is_empty());
}

/// A rename onto a path the session already knows collides: re-pointing
/// would orphan the existing path's facts, so the decision is a skip —
/// never a half-applied rename.
#[test]
fn a_rename_onto_a_known_path_answers_nothing() {
    let source = MockSource {
        known: vec![PathBuf::from("/ws/main.g"), PathBuf::from("/ws/lib.g")],
        owned: vec![PathBuf::from("/ws/main.g"), PathBuf::from("/ws/lib.g")],
    };
    let decisions = compute_rename_decisions(&[rename("/ws/main.g", "/ws/lib.g")], &source);
    assert!(
        decisions.rekey.is_empty(),
        "the target path is already known"
    );
    assert!(decisions.refresh.is_empty());
}

/// One burst may repeat an old path (a client reporting overlapping
/// operations): the first decision wins and the duplicates collapse, so
/// one rename burst answers at most one action per old path.
#[test]
fn duplicate_old_paths_collapse_to_the_first_decision() {
    let source = MockSource {
        known: vec![PathBuf::from("/ws/main.g")],
        owned: vec![PathBuf::from("/ws/main.g")],
    };
    let decisions = compute_rename_decisions(
        &[
            rename("/ws/main.g", "/ws/a.g"),
            rename("/ws/main.g", "/ws/b.g"),
        ],
        &source,
    );
    assert_eq!(decisions.rekey, vec![rename("/ws/main.g", "/ws/a.g")]);
}

/// Event order is preserved inside each decision list: the route
/// executes the actions in the order the client reported them.
#[test]
fn event_order_is_preserved() {
    let source = MockSource {
        known: vec![
            PathBuf::from("/ws/open.g"),
            PathBuf::from("/ws/watched.g"),
            PathBuf::from("/ws/open2.g"),
        ],
        owned: vec![PathBuf::from("/ws/open.g"), PathBuf::from("/ws/open2.g")],
    };
    let decisions = compute_rename_decisions(
        &[
            rename("/ws/open.g", "/ws/open-new.g"),
            rename("/ws/watched.g", "/ws/watched-new.g"),
            rename("/ws/open2.g", "/ws/open2-new.g"),
        ],
        &source,
    );
    assert_eq!(
        decisions
            .rekey
            .iter()
            .map(|r| r.old.file_name().unwrap().to_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["open.g", "open2.g"],
        "rekeys answer in event order"
    );
    assert_eq!(decisions.refresh.len(), 1);
    assert_eq!(decisions.refresh[0].old, PathBuf::from("/ws/watched.g"));
}

/// A path-less rename names no file and decides nothing — the same
/// honest skip a path-less watched event gets.
#[test]
fn a_pathless_rename_answers_nothing() {
    let source = MockSource {
        known: vec![],
        owned: vec![],
    };
    let decisions = compute_rename_decisions(
        &[FileRename {
            old: PathBuf::new(),
            new: PathBuf::from("/ws/x.g"),
        }],
        &source,
    );
    assert!(decisions.rekey.is_empty());
    assert!(decisions.refresh.is_empty());
}
