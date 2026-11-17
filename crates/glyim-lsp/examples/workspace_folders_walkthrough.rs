//! Consumer walkthrough of the workspace-folder seam
//! (LSP-WORKSPACE-FOLDERS): the folder set the session records from the
//! client's initialize params and re-derives on
//! `workspace/didChangeWorkspaceFolders`.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example workspace_folders_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: the initialize seed records the client's folder list (the
//! deprecated `rootUri` is the fallback when none is sent — unresolvable
//! URIs are skipped, never invented), the change route re-derives the set
//! over one client event (known removals drop, unknown removals skip,
//! re-adds keep their first recording), and the folder queries —
//! `recorded_folders` and `folders_containing` — answer off the store,
//! no analysis work involved. Folders are session metadata: nothing is
//! re-analysed, nothing is re-parsed.

use glyim_lsp::session_folders::{
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

fn main() {
    // The client's initialize: two file-scheme folders and one the
    // session cannot key (`untitled:`) — the seed skips that one.
    let db = glyim_lsp::AnalysisDatabase::new();
    let params = InitializeParams {
        process_id: None,
        root_path: None,
        root_uri: None,
        initialization_options: None,
        capabilities: ClientCapabilities::default(),
        trace: None,
        workspace_folders: Some(vec![
            wire_folder("file:///walkthrough/site", "site"),
            wire_folder("untitled:Untitled-1", "scratch"),
            wire_folder("file:///walkthrough/site/docs", "docs"),
        ]),
        client_info: None,
        locale: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let seeded = seed_workspace_folders(&db, &params);
    println!("workspace_folders_walkthrough: seeded {seeded} folder(s) from initialize");
    for folder in recorded_folders(&db) {
        println!("  serves {} as {:?}", folder.path.display(), folder.name);
    }

    // The client's change event: remove one folder (known), remove one
    // the session never knew, add a new one.
    did_change_workspace_folders(
        &db,
        &DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![wire_folder("file:///walkthrough/extra", "extra")],
                removed: vec![
                    wire_folder("file:///walkthrough/site/docs", "docs"),
                    wire_folder("file:///walkthrough/never-known", "gone"),
                ],
            },
        },
    );
    println!("after the change event:");
    for folder in recorded_folders(&db) {
        println!("  serves {} as {:?}", folder.path.display(), folder.name);
    }

    // The scoping query: which recorded folders contain a file.
    let query = PathBuf::from("/walkthrough/site/src/main.g");
    let containing = folders_containing(&db, &query);
    println!("folders containing {}:", query.display());
    for folder in containing {
        println!("  {:?}", folder.name);
    }
    assert!(folders_containing(&db, &PathBuf::from("/elsewhere/main.g")).is_empty());
    println!("  the folder set is session metadata — no analysis work involved");
}
