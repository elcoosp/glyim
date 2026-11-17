//! Consumer walkthrough of the file-rename seam (LSP-FILE-RENAME): the
//! `workspace/didRenameFiles` notification behind a client's rename UI.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example file_rename_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a session is populated exactly like the analysis driver
//! keeps it (the editor owns one document whose facts are keyed by the
//! `FileId`, the server has re-analysed one generated file through
//! watching, and the open document depends on the generated file), and
//! `renamed_file_decisions` — the function the router registers for the
//! notification — turns one burst of wire `FileRename` pairs into the
//! re-key/refresh decisions the route executes. A re-key feeds the
//! driver's `FileRenamed` message (the path maps move, the facts keyed
//! by the id survive); a refresh is the watched-files honest refresh
//! over the new path. The walkthrough prints the decisions it would
//! feed and spells out the driver-side effect of each.

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::file_rename::renamed_file_decisions;
use lsp_types::{FileRename, RenameFilesParams};
use std::path::PathBuf;
use url::Url;

fn main() {
    // Populate the session exactly like the driver keeps it: the editor
    // owns `main.g` (file map + recorded facts, no watched record), the
    // server loaded `generated.g` through watching (file map + watched
    // record), and `main.g` depends on `generated.g` (the driver's dep
    // edge — a re-key must move it with the file).
    let db = AnalysisDatabase::new();
    db.file_map
        .write()
        .get_or_create(&PathBuf::from("/walkthrough/main.g"));
    db.file_map
        .write()
        .get_or_create(&PathBuf::from("/walkthrough/generated.g"));
    db.watched_files
        .write()
        .insert(PathBuf::from("/walkthrough/generated.g"));
    db.dep_graph.write().add_dep(
        PathBuf::from("/walkthrough/main.g"),
        PathBuf::from("/walkthrough/generated.g"),
    );

    // One burst of client-reported renames: the editor-owned document
    // moves, the watched file moves with it, and a stranger the session
    // never knew is reported too (clients report workspace-wide).
    let uri = |p: &str| Url::from_file_path(p).unwrap().to_string();
    let params = RenameFilesParams {
        files: vec![
            FileRename {
                old_uri: uri("/walkthrough/main.g"),
                new_uri: uri("/walkthrough/lib.g"),
            },
            FileRename {
                old_uri: uri("/walkthrough/generated.g"),
                new_uri: uri("/walkthrough/generated_v2.g"),
            },
            FileRename {
                old_uri: uri("/walkthrough/stranger.g"),
                new_uri: uri("/walkthrough/stranger_v2.g"),
            },
        ],
    };

    let decisions = renamed_file_decisions(&db, &params);
    println!("file_rename_walkthrough: one burst, three reported renames");
    for rename in &decisions.rekey {
        println!(
            "  re-key {} -> {} — feed the driver's FileRenamed message: \
             the path maps move, the FileId and every fact keyed by it \
             survive, the dep edges follow, and the diagnostics report \
             republishes under the new path",
            rename.old.display(),
            rename.new.display()
        );
    }
    for rename in &decisions.refresh {
        println!(
            "  refresh {} -> {} — forget the old path (the close cleanup \
             clears the recorded facts) and re-analyse the new one from \
             disk: the watched seam's own honest refresh",
            rename.old.display(),
            rename.new.display()
        );
    }
    assert!(
        decisions
            .rekey
            .contains(&glyim_lsp::file_rename::FileRename {
                old: PathBuf::from("/walkthrough/main.g"),
                new: PathBuf::from("/walkthrough/lib.g"),
            }),
        "the editor-owned rename re-keys in place"
    );
    assert!(
        decisions
            .refresh
            .contains(&glyim_lsp::file_rename::FileRename {
                old: PathBuf::from("/walkthrough/generated.g"),
                new: PathBuf::from("/walkthrough/generated_v2.g"),
            }),
        "the watched rename refreshes from disk"
    );
    println!(
        "  the stranger is skipped — the session has no facts to move \
         for a path it does not know"
    );
}
