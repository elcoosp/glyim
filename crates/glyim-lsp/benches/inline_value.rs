//! Inline-value request bench (LSP-INLINE-VALUE).
//!
//! Dependency-free (`harness = false`), mirroring the `moniker` bench:
//! measures the inline-value request path end-to-end over a synthetic
//! binding-heavy corpus — the recorded-binding walk over the stored
//! `CrateHir`s (`let` expressions with their byte-exact pattern spans)
//! and the range-overlap policy — so CI logs can track the per-request
//! cost as the inline-value seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench inline_value
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_core::{CrateId, Interner};
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::inline_value::inline_values;
use lsp_types::{InlineValueParams, Position, Range, TextDocumentIdentifier, Uri};
use url::Url;

/// One unit: one function with several `let` bindings — the realistic
/// mix a debugger frame shows. The bindings scale the recorded walk
/// while the range stays fixed.
const fn unit_source() -> &'static str {
    "fn frame() -> i32 {\n\
     \x20   let a = 1;\n\
     \x20   let b = a + 1;\n\
     \x20   let c = b + 1;\n\
     \x20   let d = c + 1;\n\
     \x20   d\n\
     }\n\n"
}
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` binding groups, analysed once through the
    // same pipeline the analysis driver runs (parse → def-map → HIR), the
    // HIRs stored exactly like the driver stores them.
    let mut source = String::with_capacity(UNITS * 110);
    for _ in 0..UNITS {
        source.push_str(unit_source());
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/inline_value.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let parse_result = parse_to_syntax(&source, file_id);
    let mut interner = Interner::new();
    let (_def_map, _def_diags) =
        glyim_def_map::build_def_map(&parse_result.root, CrateId::from_raw(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();

    // Warm the pipeline facts once outside the measured loop: the bench
    // measures the request path, not the analysis driver.
    let params_at = |start_line: u32, end_line: u32| InlineValueParams {
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
        context: lsp_types::InlineValueContext {
            frame_id: 0,
            stopped_location: Range {
                start: Position {
                    line: start_line,
                    character: 0,
                },
                end: Position {
                    line: start_line,
                    character: 0,
                },
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    let lines = source.lines().count() as u32;
    inline_values(&db, &file_map, &params_at(0, lines)).expect("the corpus records bindings");

    // The measured section: per-request cost over the stored facts.
    let requests = 200;
    let paths: [(&str, u32, u32); 2] = [
        ("whole-corpus range (every binding)", 0, lines),
        ("one-frame range (one unit's bindings)", 0, 7),
    ];
    println!("inline_value bench: {UNITS} binding units, {requests} requests per path");
    for (label, start_line, end_line) in paths {
        let params = params_at(start_line, end_line);
        let start = Instant::now();
        for _ in 0..requests {
            black_box(inline_values(&db, &file_map, black_box(&params)));
        }
        let elapsed = start.elapsed();
        println!(
            "  {label:<38} {:>10.1?} total, {:>8.1?}/request",
            elapsed,
            elapsed / requests
        );
    }
}
