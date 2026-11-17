//! Consumer walkthrough of the file create/delete seam
//! (LSP-FILE-CREATE-DELETE): the `workspace/didCreateFiles` and
//! `workspace/didDeleteFiles` notifications behind a client's workspace
//! UI.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example file_operations_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a session is populated exactly like the analysis driver
//! keeps it (the editor owns one document, the server has re-analysed
//! one generated file through watching), and the two decision entry
//! points the router registers — `created_file_decisions` and
//! `deleted_file_decisions` — turn the declared operations into the
//! reanalyse/forget decisions the routes execute. The decisions reuse
//! the watched-files policy on purpose: the declared shape is the
//! forwarded events' counterpart, and one ownership rule decides both.
//! The walkthrough prints the decisions it would feed and spells out
//! the driver-side effect of each.

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::file_operations::{created_file_decisions, deleted_file_decisions};
use lsp_types::{CreateFilesParams, DeleteFilesParams, FileCreate, FileDelete};
use std::path::PathBuf;
use url::Url;

fn main() {
    // Populate the session exactly like the driver keeps it: the editor
    // owns `main.g` (file map, no watched record), the server loaded
    // `generated.g` through watching (file map + watched record).
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

    let uri = |p: &str| Url::from_file_path(p).unwrap().to_string();

    // One create burst: the editor's explorer created two files — a
    // stranger the session never knew and the editor-owned document
    // again (a client may echo the buffer it owns).
    let created = created_file_decisions(
        &db,
        &CreateFilesParams {
            files: vec![
                FileCreate {
                    uri: uri("/walkthrough/new_module.g"),
                },
                FileCreate {
                    uri: uri("/walkthrough/main.g"),
                },
            ],
        },
    );
    println!("file_operations_walkthrough: didCreateFiles with two entries");
    for path in &created.reanalyse {
        println!(
            "  reanalyse {} — read from disk and feed the driver; the \
             file's definitions may resolve bare usages in the open \
             documents",
            path.display()
        );
    }
    assert!(
        created
            .reanalyse
            .contains(&PathBuf::from("/walkthrough/new_module.g")),
        "the unknown create re-analyses"
    );
    println!(
        "  the editor-owned create is skipped — the client's didOpen \
         owns the buffer's content"
    );

    // One delete burst: the explorer deleted the watched load and the
    // editor-owned document again.
    let deleted = deleted_file_decisions(
        &db,
        &DeleteFilesParams {
            files: vec![
                FileDelete {
                    uri: uri("/walkthrough/generated.g"),
                },
                FileDelete {
                    uri: uri("/walkthrough/main.g"),
                },
            ],
        },
    );
    println!("file_operations_walkthrough: didDeleteFiles with two entries");
    for path in &deleted.forget {
        println!(
            "  forget {} — reuse the close cleanup: recorded facts \
             cleared, an empty diagnostics report published",
            path.display()
        );
    }
    assert!(
        deleted
            .forget
            .contains(&PathBuf::from("/walkthrough/generated.g")),
        "the watched delete is forgotten"
    );
    println!(
        "  the editor-owned delete is skipped — didClose owns the \
         lifecycle"
    );
}
