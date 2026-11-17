//! Database-backed tests for the workspace-folder requests
//! (LSP-WORKSPACE-FOLDERS).
//!
//! Mirrors `will_save_tests.rs`: the shared `AnalysisDatabase` store is
//! populated exactly like the initialize seed and the change route
//! populate it, and the public entry points are exercised end-to-end
//! (`seed_workspace_folders` over the client's initialize params with the
//! `rootUri` fallback; `did_change_workspace_folders` over the wire event;
//! `recorded_folders` / `folders_containing` over the store). The folder
//! store is session metadata — no analysis artifacts hang off it, so the
//! fixtures populate exactly what the engine reads.

use crate::AnalysisDatabase;
use crate::session_folders::{
    did_change_workspace_folders, folders_containing, recorded_folders, seed_workspace_folders,
};
use lsp_types::{
    ClientCapabilities, DidChangeWorkspaceFoldersParams, InitializeParams, Uri,
    WorkDoneProgressParams, WorkspaceFolder, WorkspaceFoldersChangeEvent,
};
use std::path::PathBuf;
use std::str::FromStr;

fn wire_folder(spelled: &str, name: &str) -> WorkspaceFolder {
    WorkspaceFolder {
        uri: Uri::from_str(spelled).unwrap(),
        name: name.to_string(),
    }
}

fn initialize_params(
    folders: Option<Vec<WorkspaceFolder>>,
    root_uri: Option<Uri>,
) -> InitializeParams {
    // `root_uri` is lsp-types-deprecated in favor of `workspace_folders`,
    // which is exactly the fallback shape this engine consumes; the
    // workspace lint table allows `deprecated` (see the root Cargo.toml).
    InitializeParams {
        process_id: None,
        root_path: None,
        root_uri,
        initialization_options: None,
        capabilities: ClientCapabilities::default(),
        trace: None,
        workspace_folders: folders,
        client_info: None,
        locale: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

fn change_params(
    added: Vec<WorkspaceFolder>,
    removed: Vec<WorkspaceFolder>,
) -> DidChangeWorkspaceFoldersParams {
    DidChangeWorkspaceFoldersParams {
        event: WorkspaceFoldersChangeEvent { added, removed },
    }
}

/// The seed records the client's folder set in first-added order; a
/// folder whose URI does not resolve to a file path is skipped with the
/// seed's honesty (never invented).
#[test]
fn the_seed_records_the_client_folder_set_in_order() {
    let db = AnalysisDatabase::new();
    let params = initialize_params(
        Some(vec![
            wire_folder("file:///work/site", "site"),
            wire_folder("untitled:Untitled-1", "scratch"),
            wire_folder("file:///work/docs", "docs"),
        ]),
        None,
    );
    let seeded = seed_workspace_folders(&db, &params);
    assert_eq!(seeded, 2, "the unresolvable folder is skipped");
    let folders = recorded_folders(&db);
    assert_eq!(folders.len(), 2);
    assert_eq!(folders[0].name, "site", "first-added order");
    assert_eq!(folders[0].path, PathBuf::from("/work/site"));
    assert_eq!(folders[1].path, PathBuf::from("/work/docs"));
}

/// A client that sends no folder list falls back to the deprecated
/// `rootUri` — the compatibility shape older clients still spell.
#[test]
fn a_root_uri_client_falls_back_to_the_root_folder() {
    let db = AnalysisDatabase::new();
    let params = initialize_params(None, Some(Uri::from_str("file:///work/legacy").unwrap()));
    let seeded = seed_workspace_folders(&db, &params);
    assert_eq!(seeded, 1, "the root becomes the one served folder");
    let folders = recorded_folders(&db);
    assert_eq!(folders[0].path, PathBuf::from("/work/legacy"));
    assert_eq!(folders[0].name, "root", "the fallback names itself");
}

/// An empty folder list falls back to the root too: "none configured"
/// means the same on both wire shapes, and the client's root still names
/// the served workspace.
#[test]
fn an_empty_folder_list_falls_back_to_the_root() {
    let db = AnalysisDatabase::new();
    let params = initialize_params(
        Some(Vec::new()),
        Some(Uri::from_str("file:///work/legacy").unwrap()),
    );
    assert_eq!(seed_workspace_folders(&db, &params), 1);
}

/// The change route re-derives the store: known removals drop, unknown
/// removals skip, re-adds are idempotent, and the surviving set keeps its
/// first-added order.
#[test]
fn a_change_event_re_derives_the_recorded_set() {
    let db = AnalysisDatabase::new();
    seed_workspace_folders(
        &db,
        &initialize_params(
            Some(vec![
                wire_folder("file:///work/site", "site"),
                wire_folder("file:///work/docs", "docs"),
            ]),
            None,
        ),
    );

    did_change_workspace_folders(
        &db,
        &change_params(
            vec![wire_folder("file:///work/extra", "extra")],
            vec![
                wire_folder("file:///work/docs", "docs"),
                wire_folder("file:///gone", "gone"),
            ],
        ),
    );
    let folders = recorded_folders(&db);
    assert_eq!(
        folders
            .iter()
            .map(|folder| folder.name.as_str())
            .collect::<Vec<_>>(),
        vec!["site", "extra"],
        "the known removal dropped, the unknown removal skipped"
    );

    // A re-add of an already-recorded folder is idempotent.
    did_change_workspace_folders(
        &db,
        &change_params(
            vec![wire_folder("file:///work/site", "renamed-site")],
            Vec::new(),
        ),
    );
    assert_eq!(
        recorded_folders(&db)[0].name,
        "site",
        "the first recording stands"
    );
}

/// Remove-then-re-add in one event re-records the folder, exactly what
/// the client spelled.
#[test]
fn remove_and_readd_in_one_event_re_records_over_the_store() {
    let db = AnalysisDatabase::new();
    seed_workspace_folders(
        &db,
        &initialize_params(Some(vec![wire_folder("file:///work/site", "site")]), None),
    );
    did_change_workspace_folders(
        &db,
        &change_params(
            vec![wire_folder("file:///work/site", "moved")],
            vec![wire_folder("file:///work/site", "site")],
        ),
    );
    assert_eq!(recorded_folders(&db)[0].name, "moved");
}

/// The scoping query answers through the store: the folders whose path
/// contains the queried path, first-added order.
#[test]
fn folders_containing_answers_through_the_store() {
    let db = AnalysisDatabase::new();
    seed_workspace_folders(
        &db,
        &initialize_params(
            Some(vec![
                wire_folder("file:///work", "root"),
                wire_folder("file:///work/site", "site"),
            ]),
            None,
        ),
    );
    let containing = folders_containing(&db, &PathBuf::from("/work/site/src/main.g"));
    assert_eq!(
        containing
            .iter()
            .map(|folder| folder.name.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "site"],
        "first-added order, both containing folders answer"
    );
    assert!(folders_containing(&db, &PathBuf::from("/opt/doc.g")).is_empty());
}

/// The wire contract the engine implements: the lsp-types notification
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_methods_are_pinned() {
    use lsp_types::notification::{DidChangeWorkspaceFolders as DidChange, Notification};
    assert_eq!(DidChange::METHOD, "workspace/didChangeWorkspaceFolders");
}
