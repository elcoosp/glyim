//! Watched-files bench (LSP-WATCHED-FILES).
//!
//! Dependency-free (`harness = false`), mirroring the other request
//! benches: measures the watched-event decision path end-to-end over a
//! synthetic session — the wire-event mapping and the ownership policy
//! over the file map and the watched set — so CI logs can track the
//! per-notification cost as the watched-files seam grows consumers.
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench watched_files
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::watched_files::watched_file_decisions;
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// One burst: 32 watched events over a session of 64 open files — the
/// realistic mix a branch switch or a build forward. The session scales
/// the ownership lookups while the burst stays fixed.
const OPEN_FILES: usize = 64;
const EVENTS: usize = 32;

fn main() {
    // Build the session: 64 editor-open files, a few watched-only paths,
    // and dep edges so the ownership lookups hit real state.
    let db = AnalysisDatabase::new();
    for i in 0..OPEN_FILES {
        db.file_map
            .write()
            .get_or_create(&PathBuf::from(format!("/bench/open_{i}.g")));
    }
    for i in 0..4 {
        db.watched_files
            .write()
            .insert(PathBuf::from(format!("/bench/generated_{i}.g")));
    }
    db.dep_graph.write().add_dep(
        PathBuf::from("/bench/open_0.g"),
        PathBuf::from("/bench/generated_0.g"),
    );

    // The burst: 32 events — mostly open files (skipped) plus the
    // watched-only ones (re-analysed) and one deletion (forgotten).
    let mut events = Vec::with_capacity(EVENTS);
    for i in 0..EVENTS {
        let path = if i % 8 == 0 {
            format!("/bench/generated_{}.g", i / 8)
        } else {
            format!("/bench/open_{}.g", i % OPEN_FILES)
        };
        let uri =
            Uri::from_str(Url::from_file_path(PathBuf::from(&path)).unwrap().as_ref()).unwrap();
        let typ = if i == EVENTS - 1 {
            FileChangeType::DELETED
        } else {
            FileChangeType::CHANGED
        };
        events.push(FileEvent::new(uri, typ));
    }
    let params = DidChangeWatchedFilesParams { changes: events };

    // Warm the session state once outside the measured loop.
    let warm = watched_file_decisions(&db, &params);
    assert!(
        !warm.reanalyse.is_empty(),
        "the burst re-analyses watched paths"
    );

    // The measured section: per-notification decision cost.
    let requests = 500;
    println!("watched_files bench: {OPEN_FILES} open files, {EVENTS} events, {requests} requests");
    let start = Instant::now();
    for _ in 0..requests {
        black_box(watched_file_decisions(&db, black_box(&params)));
    }
    let elapsed = start.elapsed();
    println!(
        "  {:<38} {:>10.1?} total, {:>8.1?}/request",
        "one watched-event burst",
        elapsed,
        elapsed / requests
    );
}
