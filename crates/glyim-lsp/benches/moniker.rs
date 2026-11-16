//! Moniker request bench (LSP-MONIKER).
//!
//! Dependency-free (`harness = false`), mirroring the `type_hierarchy`
//! bench: measures the moniker request paths end-to-end over a synthetic
//! import-heavy corpus — the cursor-on-import path (the spelled-path walk
//! over every `use` statement in the parse tree), the cursor-on-definition
//! path (the recorded-extent containment), and the two bare-usage paths
//! (name-keyed index walks for the import and export identities) through
//! the `MonikerSource` seam over the driver's stored `SymbolIndex` — so
//! CI logs can track the per-request cost as the moniker seam grows
//! consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench moniker
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_core::{CrateId, Interner};
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::moniker::monikers;
use lsp_types::{
    MonikerParams, Position, TextDocumentIdentifier, TextDocumentPositionParams, Uri,
    WorkDoneProgressParams,
};
use url::Url;

/// One unit: one import binding, one struct, one free function, and one
/// body using both the imported name and the recorded one — the
/// realistic mix an editor session shows. The `use` statements scale the
/// spelled-path walk while the index scales the name-keyed walks.
fn unit_source(i: usize) -> String {
    format!(
        "use crate::shapes::Point{i};\n\
         struct Unit{i} {{ v: i32 }}\n\
         fn helper{i}() -> i32 {{ 1 }}\n\
         fn main{i}() -> i32 {{\n\
         \x20   let p = Point{i} {{ x: 1, y: 2 }};\n\
         \x20   helper{i}()\n\
         }}\n\n"
    )
}
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` import/definition groups, analysed once
    // through the same pipeline the analysis driver runs (parse → def-map
    // → HIR → symbol index), the stores filled exactly like the driver
    // fills them.
    let mut source = String::with_capacity(UNITS * 160);
    for i in 0..UNITS {
        source.push_str(&unit_source(i));
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/moniker.gly");
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
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();
    // The first unit's anchors: the binding inside its `use` statement,
    // the struct's name at its definition, and the two bare usages in
    // its `main`.
    let cursor_on_use = source.find("crate::shapes::Point0").unwrap() + 16;
    let cursor_on_struct = source.find("struct Unit0").unwrap() + 7;
    let cursor_on_import_usage = source.find("Point0 { x: 1, y: 2 }").unwrap() + 2;
    let cursor_on_recorded_usage = source.find("helper0()").unwrap() + 3;

    // Warm the pipeline facts once outside the measured loop: the bench
    // measures the request paths, not the analysis driver.
    let params_at = |offset: usize| {
        let (line, character) = offset_to_line_col(&source, offset);
        MonikerParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        }
    };
    monikers(&db, &file_map, &params_at(cursor_on_use))
        .expect("the first unit's import binding answers");

    // The measured section: per-request cost over the stored facts.
    let requests = 200;
    let paths: [(&str, usize); 4] = [
        ("on the use binding (spelled-path walk)", cursor_on_use),
        ("on the definition (extent containment)", cursor_on_struct),
        ("bare usage of the imported name", cursor_on_import_usage),
        ("bare usage of a recorded name", cursor_on_recorded_usage),
    ];
    println!("moniker bench: {UNITS} import/definition units, {requests} requests per path");
    for (label, offset) in paths {
        let params = params_at(offset);
        let start = Instant::now();
        for _ in 0..requests {
            black_box(monikers(&db, &file_map, black_box(&params)));
        }
        let elapsed = start.elapsed();
        println!(
            "  {label:<40} {:>10.1?} total, {:>8.1?}/request",
            elapsed,
            elapsed / requests
        );
    }
}

/// Byte offset → the `(line, utf16_col)` wire position (the same mapping
/// `SourceMap` performs on the request path).
fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    (
        line,
        source[last_nl..offset]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>() as u32,
    )
}
