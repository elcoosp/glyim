//! Type-hierarchy request bench (LSP-TYPE-HIERARCHY).
//!
//! Dependency-free (`harness = false`), mirroring the `code_lens` bench:
//! measures the three type-hierarchy request paths end-to-end over a
//! synthetic impl-heavy corpus — the recorded-item choice (prepare), the
//! trait-impl walk (supertypes), and the echo re-join plus impl-self-type
//! walk (subtypes) through the `TypeHierarchySource` seam over the
//! driver's stored `CrateHir`s — so CI logs can track the per-request
//! cost as the hierarchy seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench type_hierarchy
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::type_hierarchy::{prepare_type_hierarchy, type_subtypes, type_supertypes};
use lsp_types::{
    Position, TextDocumentIdentifier, TextDocumentPositionParams, TypeHierarchyPrepareParams,
    TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Uri, WorkDoneProgressParams,
};
use url::Url;

/// One unit: one trait and one implementor (struct + trait impl) — the
/// realistic mix an editor session shows. The impl facts scale with the
/// corpus while each hierarchy walk fans out from one recorded item.
const UNIT_SOURCE: &str = "trait Measure {\n\
                           fn magnitude(&self) -> i32;\n\
                           }\n\
                           struct Unit { v: i32 }\n\
                           impl Measure for Unit {\n\
                           fn magnitude(&self) -> i32 { self.v }\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` trait/impl groups, analysed once through
    // the same pipeline the analysis driver runs (parse → def-map → HIR),
    // the HIRs stored exactly like the driver stores them.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/type_hierarchy.gly");
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
        build_def_map(&parse_result.root, CrateId::from_usize(0), interner.clone());
    let (hir, _hir_diags) = lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    let uri = Uri::from_str(Url::from_file_path(&path).unwrap().as_ref()).unwrap();
    let cursor_on_trait = source.find("trait Measure").unwrap() + 6;
    let cursor_on_struct = source.find("struct Unit").unwrap() + 7;

    // Warm the pipeline facts once outside the measured loop: the bench
    // measures the request paths, not the analysis driver.
    let (line, character) = offset_to_line_col(&source, cursor_on_trait);
    let prepare_params = TypeHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let prepared = prepare_type_hierarchy(&db, &file_map, &prepare_params)
        .expect("the trait item prepares")
        .into_iter()
        .next()
        .unwrap();

    // The measured section: per-request cost over the stored facts.
    let requests = 200;
    let start = Instant::now();
    for _ in 0..requests {
        black_box(prepare_type_hierarchy(
            &db,
            &file_map,
            black_box(&prepare_params),
        ));
    }
    let prepare_elapsed = start.elapsed();

    let supertypes_params = TypeHierarchySupertypesParams {
        item: prepared.clone(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let start = Instant::now();
    for _ in 0..requests {
        black_box(type_supertypes(
            &db,
            &file_map,
            black_box(&supertypes_params),
        ));
    }
    let supertypes_elapsed = start.elapsed();

    let subtypes_params = TypeHierarchySubtypesParams {
        item: prepared,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let start = Instant::now();
    for _ in 0..requests {
        black_box(type_subtypes(&db, &file_map, black_box(&subtypes_params)));
    }
    let subtypes_elapsed = start.elapsed();

    // A struct echo exercises the supertypes walk with the recorded
    // classification join (the trait echo answers null by contract).
    let (line, character) = offset_to_line_col(&source, cursor_on_struct);
    let struct_params = TypeHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let struct_item = prepare_type_hierarchy(&db, &file_map, &struct_params)
        .expect("the struct item prepares")
        .into_iter()
        .next()
        .unwrap();
    let struct_supertypes_params = TypeHierarchySupertypesParams {
        item: struct_item,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let start = Instant::now();
    for _ in 0..requests {
        black_box(type_supertypes(
            &db,
            &file_map,
            black_box(&struct_supertypes_params),
        ));
    }
    let struct_supertypes_elapsed = start.elapsed();

    println!("type_hierarchy bench: {UNITS} trait/impl units, {requests} requests per path");
    println!(
        "  prepareTypeHierarchy:      {:>10.1?} total, {:>8.1?}/request",
        prepare_elapsed,
        prepare_elapsed / requests
    );
    println!(
        "  supertypes (trait echo):   {:>10.1?} total, {:>8.1?}/request  (echo join + null answer)",
        supertypes_elapsed,
        supertypes_elapsed / requests
    );
    println!(
        "  subtypes (trait echo):     {:>10.1?} total, {:>8.1?}/request",
        subtypes_elapsed,
        subtypes_elapsed / requests
    );
    println!(
        "  supertypes (struct echo):  {:>10.1?} total, {:>8.1?}/request  (impl walk)",
        struct_supertypes_elapsed,
        struct_supertypes_elapsed / requests
    );
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
