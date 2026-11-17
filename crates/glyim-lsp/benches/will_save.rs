//! Save-time edit bench (LSP-WILL-SAVE).
//!
//! Dependency-free (`harness = false`), mirroring the `range_formatting`
//! and `inline_value` benches: measures the save-request path end-to-end
//! over a synthetic partially-formatted corpus — the shared formatter
//! core's saved form, the line-with-terminator diff, and the wire range
//! mapping — so CI logs can track the per-request cost as the save seam
//! grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench will_save
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::will_save::will_save_edits;
use lsp_types::{TextDocumentIdentifier, TextDocumentSaveReason, Uri, WillSaveTextDocumentParams};
use url::Url;

/// One unit: a brace-free, already-formatted statement block — the
/// fixpoint region the save keeps outside the edit. The units scale the
/// unchanged head and tail the diff walks past while the drifted middle
/// line stays fixed.
const fn unit_source() -> &'static str {
    "let unit = 1;\n"
}
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` formatted statement lines, one drifted
    // middle line, the same units again — a realistic "small edit left
    // one line unformatted" save. Stored exactly like the driver stores
    // it (the save engine reads the stored text only).
    let mut source = String::with_capacity(UNITS * 14 * 2 + 24);
    for _ in 0..UNITS {
        source.push_str(unit_source());
    }
    source.push_str("  let drifted = 2;\n");
    for _ in 0..UNITS {
        source.push_str(unit_source());
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/will_save.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();
    let params = WillSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        reason: TextDocumentSaveReason::MANUAL,
    };

    // Warm the stored facts once outside the measured loop: the bench
    // measures the save-request path, not the analysis driver.
    will_save_edits(&db, &file_map, &params).expect("the drifted line needs an edit");

    // The measured section: per-request cost over the stored text — one
    // drifted middle and one already-formatted whole (the no-churn path).
    let requests = 200;
    let formatted_only = {
        let mut s = String::with_capacity(UNITS * 14 * 2);
        for _ in 0..(UNITS * 2) {
            s.push_str(unit_source());
        }
        s
    };
    let formatted_path = PathBuf::from("/bench/will_save_formatted.gly");
    let formatted_id = db.file_map.write().get_or_create(&formatted_path);
    let formatted_caller_id = file_map.get_or_create(&formatted_path);
    assert_eq!(formatted_id, formatted_caller_id, "file ids must agree");
    db.source_maps.write().insert(
        formatted_id,
        SourceMap::new(formatted_id, formatted_only.clone()),
    );
    let formatted_uri =
        Uri::from_str(Url::from_file_path(&formatted_path).unwrap().as_ref()).unwrap();
    let formatted_params = WillSaveTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: formatted_uri.clone(),
        },
        reason: TextDocumentSaveReason::MANUAL,
    };

    let paths: [(&str, &WillSaveTextDocumentParams); 2] = [
        ("one drifted line (one edit)", &params),
        ("already formatted (null)", &formatted_params),
    ];
    println!("will_save bench: {UNITS} statement units, {requests} requests per path");
    for (label, request) in paths {
        let start = Instant::now();
        for _ in 0..requests {
            black_box(will_save_edits(&db, &file_map, black_box(request)));
        }
        let elapsed = start.elapsed();
        println!(
            "  {label:<38} {:>10.1?} total, {:>8.1?}/request",
            elapsed,
            elapsed / requests
        );
    }
}
