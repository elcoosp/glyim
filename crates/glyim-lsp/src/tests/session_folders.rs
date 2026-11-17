//! Policy-level tests for the workspace-folder engine
//! (LSP-WORKSPACE-FOLDERS).
//!
//! Mirrors `file_rename.rs` and `will_save.rs`: the *policy* (which set
//! the session records after one change event, which folders contain a
//! path) is pinned through wire-free records and pure functions,
//! decoupled from any database or lock. The store-backed production path
//! and the full wire shapes are covered end-to-end in
//! `session_folders_tests.rs`.

use crate::session_folders::{
    FolderChange, FolderRecord, FolderSetSource, compute_folder_set, compute_folders_containing,
    folder_record_from_wire,
};
use std::path::PathBuf;

/// A record for `path`, named after its final component — the shape the
/// policy tests build by hand.
fn record(path: &str, name: &str) -> FolderRecord {
    FolderRecord {
        name: name.to_string(),
        uri: format!("file://{path}"),
        path: PathBuf::from(path),
    }
}

/// A file-scheme folder converts to its path-keyed record, name and URI
/// kept verbatim for round-trips.
#[test]
fn a_file_uri_folder_converts_to_a_path_keyed_record() {
    use std::str::FromStr;
    let folder = lsp_types::WorkspaceFolder {
        uri: lsp_types::Uri::from_str("file:///work/site").unwrap(),
        name: "site".to_string(),
    };
    let converted = folder_record_from_wire(&folder).expect("a file URI converts");
    assert_eq!(converted.path, PathBuf::from("/work/site"));
    assert_eq!(converted.name, "site");
    assert_eq!(converted.uri, "file:///work/site");
}

/// A folder the session cannot key (non-file scheme) converts to `None` —
/// the session is path-keyed end-to-end, so a remote or `untitled:` URI
/// is skipped, never invented.
#[test]
fn a_non_file_uri_folder_converts_to_none() {
    use std::str::FromStr;
    for spelled in [
        "untitled:Untitled-1",
        "ftp://host/work",
        "https://host/work",
    ] {
        let folder = lsp_types::WorkspaceFolder {
            uri: lsp_types::Uri::from_str(spelled).unwrap(),
            name: spelled.to_string(),
        };
        assert!(
            folder_record_from_wire(&folder).is_none(),
            "`{spelled}` must not convert"
        );
    }
}

/// A removal the session knows drops exactly that folder; an unknown
/// removal is skipped — a refusal must not reorder or truncate the
/// surviving set.
#[test]
fn a_change_event_applies_removals_honestly() {
    let recorded = vec![record("/work/a", "a"), record("/work/b", "b")];
    let change = FolderChange {
        added: Vec::new(),
        removed: vec![record("/work/b", "b"), record("/work/gone", "gone")],
    };
    let next = compute_folder_set(&recorded, &change);
    assert_eq!(
        next,
        vec![record("/work/a", "a")],
        "the known removal drops, the unknown removal skips"
    );
}

/// A re-add of an already-recorded folder is idempotent: the first
/// recording wins — its position and name stand.
#[test]
fn a_re_added_folder_keeps_its_first_recording() {
    let recorded = vec![record("/work/a", "a"), record("/work/b", "b")];
    let change = FolderChange {
        added: vec![record("/work/a", "renamed-a")],
        removed: Vec::new(),
    };
    let next = compute_folder_set(&recorded, &change);
    assert_eq!(
        next,
        vec![record("/work/a", "a"), record("/work/b", "b")],
        "the first recording stands"
    );
}

/// Within one event, removals and additions apply in event order:
/// `remove(A); add(A)` re-records A — exactly what the client spelled.
#[test]
fn remove_and_readd_in_one_event_re_records() {
    let recorded = vec![record("/work/a", "a")];
    let change = FolderChange {
        added: vec![record("/work/a", "moved")],
        removed: vec![record("/work/a", "a")],
    };
    let next = compute_folder_set(&recorded, &change);
    assert_eq!(
        next,
        vec![record("/work/a", "moved")],
        "the client's remove-then-add re-records the folder"
    );
}

/// The folder-of-path query: the folders whose path contains the queried
/// path answer in first-added order, nested folders included, and a
/// query for a folder path itself answers that folder.
#[test]
fn folders_containing_answers_first_added_order() {
    use std::path::Path;
    let recorded = vec![
        record("/work", "root"),
        record("/elsewhere", "other"),
        record("/work/site", "site"),
    ];
    let inside = compute_folders_containing(&recorded, Path::new("/work/site/src/main.g"));
    assert_eq!(
        inside,
        vec![record("/work", "root"), record("/work/site", "site")],
        "first-added order, both containing folders answer"
    );
    let outside = compute_folders_containing(&recorded, Path::new("/elsewhere/doc.g"));
    assert_eq!(outside.len(), 1, "only the containing folder answers");
    let itself = compute_folders_containing(&recorded, Path::new("/work"));
    assert_eq!(
        itself.len(),
        1,
        "the folder path answers through itself (a nested folder does not contain the parent)"
    );
    let nowhere = compute_folders_containing(&recorded, Path::new("/opt/doc.g"));
    assert!(nowhere.is_empty(), "an uncontained path answers none");
}

/// The read seam: a source over an in-memory set answers the recorded
/// folders verbatim — the queries read through the trait, so the
/// production database source is swappable.
#[test]
fn the_read_seam_answers_the_recorded_set() {
    struct MockSource {
        folders: Vec<FolderRecord>,
    }
    impl FolderSetSource for MockSource {
        fn recorded_folders(&self) -> Vec<FolderRecord> {
            self.folders.clone()
        }
    }
    let mock = MockSource {
        folders: vec![record("/work/a", "a")],
    };
    assert_eq!(
        mock.recorded_folders(),
        vec![record("/work/a", "a")],
        "the seam answers the recorded set verbatim"
    );
}
