//! Range-formatting request bench (LSP-RANGE-FORMAT).
//!
//! Dependency-free (`harness = false`), mirroring the `inline_value`
//! bench: measures the range-formatting request path end-to-end over a
//! synthetic corpus — the claimed-line expansion, the prefix-depth
//! derivation, the shared `format_source` core, and the UTF-16 wire
//! mapping — so CI logs can track the per-request cost as the
//! range-formatting seam grows consumers. The engine reads the stored
//! document text only, so the corpus populates just the source map (no
//! HIR pass is on its request path). Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench range_formatting
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::range_formatting::range_formatting;
use lsp_types::{DocumentRangeFormattingParams, Position, Range, TextDocumentIdentifier, Uri};
use url::Url;

/// One unit: one function with a compressed statement and an already
/// formatted one — the realistic mix a selection touches. The units scale
/// the claimed span's prefix scan while the per-request shape stays fixed.
const fn unit_source() -> &'static str {
    "fn unit() -> i32 {\n\
     \x20   let a=1;\n\
     \x20   let b = 2;\n\
     \x20   a\n\
     }\n\n"
}
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` function blocks stored exactly like the
    // driver stores them (file map + source map — the range-formatting
    // engine reads the document text only).
    let mut source = String::with_capacity(UNITS * 90);
    for _ in 0..UNITS {
        source.push_str(unit_source());
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/range_formatting.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();

    // Warm the stored facts once outside the measured loop: the bench
    // measures the request path, not the analysis driver.
    let params_at = |start_line: u32, end_line: u32| DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: start_line,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: 0,
            },
        },
        options: lsp_types::FormattingOptions::default(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    let lines = source.lines().count() as u32;
    range_formatting(&db, &file_map, &params_at(0, lines))
        .expect("the corpus has compressed lines to reformat");

    // The measured section: per-request cost over the stored text.
    let requests = 200;
    let paths: [(&str, u32, u32); 2] = [
        ("whole-corpus range (every unit)", 0, lines),
        ("one-unit range (one function block)", 0, 5),
    ];
    println!("range_formatting bench: {UNITS} units, {requests} requests per path");
    for (label, start_line, end_line) in paths {
        let params = params_at(start_line, end_line);
        let start = Instant::now();
        for _ in 0..requests {
            black_box(range_formatting(&db, &file_map, black_box(&params)));
        }
        let elapsed = start.elapsed();
        println!(
            "  {label:<38} {:>10.1?} total, {:>8.1?}/request",
            elapsed,
            elapsed / requests
        );
    }
}
