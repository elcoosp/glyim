//! File-rename bench (LSP-FILE-RENAME).
//!
//! Dependency-free (`harness = false`), mirroring the `watched_files`
//! and `will_save` benches: measures the rename-decision path
//! end-to-end over a synthetic session corpus — the wire URI mapping,
//! the ownership policy over the session's file map and watched set,
//! and the per-burst dedup — so CI logs can track the per-request cost
//! as the rename seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench file_rename
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::file_rename::renamed_file_decisions;
use lsp_types::{FileRename, RenameFilesParams};
use url::Url;

/// One editor-owned session document: the file map knows it, the
/// watched set does not. The units scale the session the policy walks.
fn unit_path(id: usize) -> String {
    format!("/bench/editor/file_{id}.g")
}
const UNITS: usize = 256;

fn main() {
    // Build the session: `units` editor-owned documents, half of them
    // with a dep edge so the graph carries realistic shape, plus a
    // handful of watched loads — the two roles the policy splits.
    let db = AnalysisDatabase::new();
    for id in 0..UNITS {
        db.file_map
            .write()
            .get_or_create(&PathBuf::from(unit_path(id)));
    }
    for id in 0..16 {
        let path = PathBuf::from(format!("/bench/watched/generated_{id}.g"));
        db.file_map.write().get_or_create(&path);
        db.watched_files.write().insert(path.clone());
    }

    // One burst: every third editor-owned file renames — the shape a
    // workspace-wide refactor reports. The engine reads the session's
    // own state, so the burst exercises the same path the route runs.
    let params = RenameFilesParams {
        files: (0..UNITS)
            .step_by(3)
            .map(|id| FileRename {
                old_uri: Url::from_file_path(unit_path(id)).unwrap().to_string(),
                new_uri: Url::from_file_path(format!("/bench/editor/renamed_{id}.g"))
                    .unwrap()
                    .to_string(),
            })
            .collect(),
    };

    // Warm the policy once outside the measured loop.
    let decisions = renamed_file_decisions(&db, &params);
    assert_eq!(
        decisions.rekey.len() + decisions.refresh.len(),
        UNITS.div_ceil(3),
        "every reported rename of a known path answers a decision"
    );

    let bursts = 200;
    println!(
        "file_rename bench: {UNITS} editor-owned + 16 watched session paths, \
         {bursts} bursts of {} renames",
        params.files.len()
    );
    let start = Instant::now();
    for _ in 0..bursts {
        black_box(renamed_file_decisions(&db, black_box(&params)));
    }
    let elapsed = start.elapsed();
    println!(
        "  {bursts} bursts of {} renames      {elapsed:>10.1?} total, {:>8.1?}/burst",
        params.files.len(),
        elapsed / bursts
    );
}
