//! Workspace-folder apply bench (LSP-WORKSPACE-FOLDERS).
//!
//! Dependency-free (`harness = false`), mirroring the `will_save` and
//! `document_color` benches: measures the change-notification path
//! end-to-end over a seeded folder store — the wire-event conversion, the
//! apply policy over the recorded set, and the store write — so CI logs
//! can track the per-notification cost as the folder seam grows
//! consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench workspace_folders
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::session_folders::{
    did_change_workspace_folders, folders_containing, recorded_folders,
};
use lsp_types::{Uri, WorkspaceFolder, WorkspaceFoldersChangeEvent};

/// The seeded folder set: `UNITS` folders under `/bench/folder-N`, the
/// size a multi-root workspace realistically carries.
const UNITS: usize = 64;

fn wire_folder(spelled: &str, name: &str) -> WorkspaceFolder {
    WorkspaceFolder {
        uri: Uri::from_str(spelled).unwrap(),
        name: name.to_string(),
    }
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    // Seed the store directly (the initialize shape), the way the driver
    // leaves it after a multi-root client connects.
    for i in 0..UNITS {
        db.workspace_folders
            .write()
            .push(glyim_lsp::session_folders::FolderRecord {
                name: format!("folder-{i}"),
                uri: format!("file:///bench/folder-{i}"),
                path: PathBuf::from(format!("/bench/folder-{i}")),
            });
    }
    assert_eq!(recorded_folders(&db).len(), UNITS);

    // The measured section: one change notification per request, the
    // churn a real client sends — two folders added, two removed. The
    // store write is the measured side effect, so the unit-returning
    // apply is called bare (the params stay black-boxed).
    let events = 200;
    for i in 0..2 {
        did_change_workspace_folders(
            &db,
            black_box(&lsp_types::DidChangeWorkspaceFoldersParams {
                event: WorkspaceFoldersChangeEvent {
                    added: vec![
                        wire_folder(&format!("file:///bench/incoming-{i}"), "incoming"),
                        wire_folder(&format!("file:///bench/incoming-b-{i}"), "incoming-b"),
                    ],
                    removed: vec![
                        wire_folder(&format!("file:///bench/folder-{i}"), "folder"),
                        wire_folder(&format!("file:///bench/folder-{}", UNITS - 1 - i), "folder"),
                    ],
                },
            }),
        );
    }
    let start = Instant::now();
    for _ in 0..events {
        did_change_workspace_folders(
            &db,
            black_box(&lsp_types::DidChangeWorkspaceFoldersParams {
                event: WorkspaceFoldersChangeEvent {
                    added: vec![wire_folder("file:///bench/ping", "ping")],
                    removed: vec![wire_folder("file:///bench/ping", "ping")],
                },
            }),
        );
    }
    let elapsed = start.elapsed();
    println!(
        "workspace_folders bench: {UNITS} seeded folders, {events} change notifications per path"
    );
    println!(
        "  one add + one remove (idempotent re-add)  {:>10.1?} total, {:>8.1?}/notification",
        elapsed,
        elapsed / events
    );

    // The read side: the folder-of-path scoping query over the store.
    let start = Instant::now();
    for _ in 0..events {
        black_box(folders_containing(
            &db,
            black_box(&PathBuf::from("/bench/folder-3/src/main.g")),
        ));
    }
    let reads = start.elapsed();
    println!(
        "  folders_containing over the store          {:>10.1?} total, {:>8.1?}/query",
        reads,
        reads / events
    );
}
