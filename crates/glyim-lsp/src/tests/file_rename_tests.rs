//! Database-backed tests for the file-rename request (LSP-FILE-RENAME).
//!
//! Mirrors `watched_files_tests.rs`: the session state populates the
//! database exactly like the analysis driver keeps it, and the public
//! `renamed_file_decisions` entry point is exercised end-to-end (URI →
//! filesystem paths → the ownership policy over the session's own
//! state). The session seams the driver's re-key executes through —
//! `FileMap::rename` and `DependencyGraph::rename_node` — are pinned
//! here too, because the re-key's whole value is *facts survive the
//! move*.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::dep_graph::DependencyGraph;
use crate::file_rename::{FileRename, renamed_file_decisions, renamed_files};
use glyim_span::FileId;
use lsp_types::notification::{DidRenameFiles as DidRenameNotification, Notification};
use lsp_types::{RenameFilesParams, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_str_for(path: &PathBuf) -> String {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
        .unwrap()
        .to_string()
}

/// Populate the session like the driver keeps it: the editor owns
/// `main.g` (file map + stored source map, no watched record), the
/// server loaded `generated.g` through watching (file map + watched
/// record), and `main.g` depends on `generated.g` (the driver's dep
/// edge — the rename must move it with the file).
fn populate_session(db: &AnalysisDatabase, file_map: &mut FileMap) -> (FileId, FileId) {
    let main_path = PathBuf::from("/test/main.g");
    let generated_path = PathBuf::from("/test/generated.g");
    let main_id = db.file_map.write().get_or_create(&main_path);
    let caller_main = file_map.get_or_create(&main_path);
    assert_eq!(main_id, caller_main, "file ids must agree");
    let generated_id = db.file_map.write().get_or_create(&generated_path);
    let caller_generated = file_map.get_or_create(&generated_path);
    assert_eq!(generated_id, caller_generated, "file ids must agree");
    db.source_maps.write().insert(
        main_id,
        SourceMap::new(main_id, "fn main() {}\n".to_string()),
    );
    db.watched_files.write().insert(generated_path.clone());
    db.dep_graph
        .write()
        .add_dep(main_path.clone(), generated_path);
    (main_id, generated_id)
}

/// The wire contracts the engine implements: the notification METHOD
/// string is pinned so an lsp-types upgrade that reshapes the endpoint
/// surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    assert_eq!(DidRenameNotification::METHOD, "workspace/didRenameFiles");
}

/// The wire events map to the wire-free facts the policy consumes: URI →
/// filesystem path, old and new kept paired. A non-`file:` URI falls
/// back to its path component — the same mapping the document-sync
/// notifications use.
#[test]
fn the_wire_events_map_to_paths() {
    let params = RenameFilesParams {
        files: vec![lsp_types::FileRename {
            old_uri: uri_str_for(&PathBuf::from("/test/main.g")),
            new_uri: uri_str_for(&PathBuf::from("/test/main_new.g")),
        }],
    };
    let renames = renamed_files(&params);
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].old, PathBuf::from("/test/main.g"));
    assert_eq!(renames[0].new, PathBuf::from("/test/main_new.g"));
}

/// The end-to-end ownership decision over the session's own state: the
/// editor-owned document re-keys in place (facts keyed by the file id
/// survive), the watched file refreshes (forget old, re-analyse new
/// from disk).
#[test]
fn the_session_answers_rekey_and_refresh_decisions() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    populate_session(&db, &mut file_map);

    let params = RenameFilesParams {
        files: vec![
            lsp_types::FileRename {
                old_uri: uri_str_for(&PathBuf::from("/test/main.g")),
                new_uri: uri_str_for(&PathBuf::from("/test/main_new.g")),
            },
            lsp_types::FileRename {
                old_uri: uri_str_for(&PathBuf::from("/test/generated.g")),
                new_uri: uri_str_for(&PathBuf::from("/test/generated_new.g")),
            },
        ],
    };
    let decisions = renamed_file_decisions(&db, &params);
    assert_eq!(
        decisions.rekey,
        vec![FileRename {
            old: PathBuf::from("/test/main.g"),
            new: PathBuf::from("/test/main_new.g"),
        }],
        "the editor-owned rename re-keys"
    );
    assert_eq!(
        decisions.refresh,
        vec![FileRename {
            old: PathBuf::from("/test/generated.g"),
            new: PathBuf::from("/test/generated_new.g"),
        }],
        "the watched rename refreshes"
    );
}

/// A rename the session knows nothing about (the path was never opened
/// and never watched) answers nothing.
#[test]
fn an_unrenamed_session_answers_nothing() {
    let db = AnalysisDatabase::new();
    let params = RenameFilesParams {
        files: vec![lsp_types::FileRename {
            old_uri: uri_str_for(&PathBuf::from("/test/stranger.g")),
            new_uri: uri_str_for(&PathBuf::from("/test/stranger_new.g")),
        }],
    };
    let decisions = renamed_file_decisions(&db, &params);
    assert!(decisions.rekey.is_empty());
    assert!(decisions.refresh.is_empty());
}

/// `FileMap::rename` preserves the `FileId` — the whole point of the
/// re-key: source maps, symbol index, reference graph, HIR, parse
/// trees, typeck state, and recorded diagnostics are all keyed by the
/// id, so the facts survive the move untouched.
#[test]
fn file_map_rename_preserves_the_file_id() {
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/a.g");
    let id = file_map.get_or_create(&path);
    let renamed = file_map.rename(&path, &PathBuf::from("/test/b.g"));
    assert!(renamed, "a known path renames");
    assert_eq!(file_map.path(id), Some(&PathBuf::from("/test/b.g")));
    assert_eq!(file_map.get_by_path(&PathBuf::from("/test/b.g")), Some(id));
    assert_eq!(file_map.get_by_path(&path), None, "the old key is gone");
    // The id counter did not move: a rename allocates nothing.
    let next = file_map.get_or_create(&PathBuf::from("/test/c.g"));
    assert_ne!(next, id, "the new file gets a fresh id");
}

/// `FileMap::rename` never half-applies: a rename of an unknown path
/// leaves the map untouched, and a rename onto an already-known path is
/// rejected (the existing key's facts must not be orphaned).
#[test]
fn file_map_rename_never_half_applies_on_a_collision() {
    let mut file_map = FileMap::new();
    let a = PathBuf::from("/test/a.g");
    let b = PathBuf::from("/test/b.g");
    let a_id = file_map.get_or_create(&a);
    let b_id = file_map.get_or_create(&b);
    assert!(
        !file_map.rename(&PathBuf::from("/test/none.g"), &b),
        "unknown old refuses"
    );
    assert_eq!(file_map.get_by_path(&a), Some(a_id), "the map is untouched");
    assert!(
        !file_map.rename(&a, &b),
        "a rename onto a known path refuses"
    );
    assert_eq!(file_map.get_by_path(&a), Some(a_id), "the old key stayed");
    assert_eq!(
        file_map.get_by_path(&b),
        Some(b_id),
        "the target kept its id"
    );
}

/// `DependencyGraph::rename_node` re-points every edge naming the old
/// path — forward (what it depends on) and reverse (what depends on it)
/// — so the driver's dependent refresh follows the file to its new
/// path instead of losing it.
#[test]
fn dep_graph_rename_repoints_every_edge() {
    let mut graph = DependencyGraph::new();
    let main = PathBuf::from("/test/main.g");
    let lib = PathBuf::from("/test/lib.g");
    let other = PathBuf::from("/test/other.g");
    // main -> lib, other -> main.
    graph.add_dep(main.clone(), lib.clone());
    graph.add_dep(other.clone(), main.clone());

    let renamed = PathBuf::from("/test/main_new.g");
    graph.rename_node(&main, &renamed);

    // The forward edge moved: main_new -> lib.
    let new_deps = graph
        .dependencies(&renamed)
        .expect("the forward edge moved");
    assert!(new_deps.contains(&lib), "the dependency follows the rename");
    assert!(graph.dependencies(&main).is_none(), "the old node is gone");
    // The reverse edge moved: other -> main_new (a change in the
    // renamed file still reaches its dependent).
    assert!(
        graph.affected_files(&renamed).contains(&other),
        "the dependent's edge re-points to the new path"
    );
    // And the old path is no longer reachable in either direction.
    assert!(
        graph.affected_files(&main).len() <= 1 && !graph.affected_files(&main).contains(&other),
        "the old path's node is fully re-pointed"
    );
}
