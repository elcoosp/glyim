//! Consumer walkthrough of the watched-files seam (LSP-WATCHED-FILES):
//! the `workspace/didChangeWatchedFiles` notification behind a client's
//! file watchers.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example watched_files_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a session is populated exactly like the analysis driver
//! keeps it (the editor owns one document, the server has re-analysed
//! one generated file through watching), and `watched_file_decisions` —
//! the function the router registers for the notification — turns one
//! burst of wire `FileEvent`s into the reanalyse/forget decisions the
//! route executes. The route itself then feeds the driver (disk read →
//! `WatchedFileChanged`, or the close cleanup for a deletion); the
//! walkthrough prints the decisions it would feed and spells out the
//! driver-side effect of each.

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::watched_files::watched_file_decisions;
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn main() {
    // Populate the session exactly like the driver keeps it: the editor
    // owns `main.g` (file map, no watched record), the server loaded
    // `generated.g` through watching (file map + watched record), and
    // `main.g` depends on `generated.g` (the driver's dep edge).
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

    // One burst of watched events: the editor-owned file changed (the
    // client's didChange owns it), the watched file changed on disk, and
    // a dependency that was deleted on disk is reported.
    let events = vec![
        FileEvent::new(
            Uri::from_str(Url::from_file_path("/walkthrough/main.g").unwrap().as_ref()).unwrap(),
            FileChangeType::CHANGED,
        ),
        FileEvent::new(
            Uri::from_str(
                Url::from_file_path("/walkthrough/generated.g")
                    .unwrap()
                    .as_ref(),
            )
            .unwrap(),
            FileChangeType::CHANGED,
        ),
        FileEvent::new(
            Uri::from_str(
                Url::from_file_path("/walkthrough/deleted_dep.g")
                    .unwrap()
                    .as_ref(),
            )
            .unwrap(),
            FileChangeType::DELETED,
        ),
    ];
    let params = DidChangeWatchedFilesParams { changes: events };

    let decisions = watched_file_decisions(&db, &params);
    println!("watched_files_walkthrough: one burst, three events");
    for path in &decisions.reanalyse {
        println!(
            "  reanalyse {} — read from disk and feed the driver; the \
             dependent refresh then re-derives every open file's facts",
            path.display()
        );
    }
    for path in &decisions.forget {
        println!(
            "  forget {} — reuse the close cleanup: recorded facts \
             cleared, an empty diagnostics report published",
            path.display()
        );
    }
    assert!(
        decisions
            .reanalyse
            .contains(&PathBuf::from("/walkthrough/generated.g"))
    );
    assert!(
        decisions
            .forget
            .contains(&PathBuf::from("/walkthrough/deleted_dep.g"))
    );
    println!("  the editor-owned change is skipped — didChange owns it");
}
