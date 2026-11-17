//! File create/delete decision bench (LSP-FILE-CREATE-DELETE).
//!
//! Dependency-free (`harness = false`), mirroring the `file_rename` and
//! `watched_files` benches: measures the declared file-operation
//! decision path end-to-end over a synthetic session corpus — the wire
//! URI mapping and the ownership policy over the session's file map and
//! watched set — so CI logs can track the per-burst cost as the
//! file-operations seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench file_operations
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use glyim_lsp::AnalysisDatabase;
use glyim_lsp::file_operations::{created_file_decisions, deleted_file_decisions};
use lsp_types::{CreateFilesParams, DeleteFilesParams, FileCreate, FileDelete};
use url::Url;

/// One editor-owned session document: the file map knows it, the
/// watched set does not. The units scale the session the policy walks.
fn unit_path(id: usize) -> String {
    format!("/bench/file_ops/file_{id}.g")
}
const UNITS: usize = 256;

fn main() {
    // Build the session: `units` editor-owned documents plus a handful
    // of watched loads — the two roles the ownership policy splits.
    let db = AnalysisDatabase::new();
    for id in 0..UNITS {
        db.file_map
            .write()
            .get_or_create(&PathBuf::from(unit_path(id)));
    }
    for id in 0..16 {
        let path = PathBuf::from(format!("/bench/file_ops/generated_{id}.g"));
        db.file_map.write().get_or_create(&path);
        db.watched_files.write().insert(path.clone());
    }

    // One create burst over the watched loads and one delete burst over
    // the same paths — the declared shapes' realistic load.
    let creates = CreateFilesParams {
        files: (0..16)
            .map(|id| FileCreate {
                uri: Url::from_file_path(format!("/bench/file_ops/generated_{id}.g"))
                    .unwrap()
                    .to_string(),
            })
            .collect(),
    };
    let deletes = DeleteFilesParams {
        files: (0..16)
            .map(|id| FileDelete {
                uri: Url::from_file_path(format!("/bench/file_ops/generated_{id}.g"))
                    .unwrap()
                    .to_string(),
            })
            .collect(),
    };

    // Warm the policies once outside the measured loop.
    let created = created_file_decisions(&db, &creates);
    let deleted = deleted_file_decisions(&db, &deletes);
    assert_eq!(created.reanalyse.len(), 16, "the watched loads re-analyse");
    assert_eq!(deleted.forget.len(), 16, "the watched loads are forgotten");

    let bursts = 500;
    println!(
        "file_operations bench: {UNITS} editor-owned + 16 watched session paths, \
         {bursts} bursts of 16 operations"
    );
    let start = Instant::now();
    for _ in 0..bursts {
        black_box(created_file_decisions(&db, black_box(&creates)));
    }
    let created_elapsed = start.elapsed();
    let start = Instant::now();
    for _ in 0..bursts {
        black_box(deleted_file_decisions(&db, black_box(&deletes)));
    }
    let deleted_elapsed = start.elapsed();
    println!(
        "  {bursts} create bursts of 16 files   {created_elapsed:>10.1?} total, {:>8.1?}/burst",
        created_elapsed / bursts
    );
    println!(
        "  {bursts} delete bursts of 16 files  {deleted_elapsed:>10.1?} total, {:>8.1?}/burst",
        deleted_elapsed / bursts
    );
}
