//! Type-definition bench (LSP-TYPE-DEF).
//!
//! Dependency-free (`harness = false`), mirroring the `selection_range` and
//! `document_highlight` benches: measures the type-definition request path
//! end-to-end over a synthetic typed corpus — the choice policy (symbol
//! selection through the `TypeSymbolSource` seam) and the full wire
//! request (offset → `type_at_offset` → `Ty` peel → name → symbol →
//! location mapping) — so CI logs can track the per-request cost as the
//! typeck seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench type_definition
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::type_definition::{SymbolIndexSource, type_definition, type_target};
use lsp_types::{
    GotoDefinitionParams, PartialResultParams, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use url::Url;

/// One shape per unit: a struct and a function touching it — enough for
/// the index to carry several same-prefix names the choice policy must
/// sift.
const UNIT_SOURCE: &str = "struct Shape { tag: i32 }\nfn use_shape(s: Shape) -> i32 { s.tag }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` structs/functions, parsed and type-checked
    // once, the database populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/type_def.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    let crate_id = glyim_core::CrateId::from_usize(0);
    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    let mut interner = glyim_core::Interner::new();
    let (def_map, _def_diags) =
        glyim_def_map::build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    let ty_ctx_mut = glyim_type::TyCtxMut::new(interner.clone());
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = glyim_solve::SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &def_map, &hir, &mut solver);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    // Cursor on the `s.tag` field read inside the LAST unit, so the
    // type query has the full corpus behind it. `use_shape` appears
    // once per unit; the field read sits inside the final one.
    let last_unit_start = (UNITS - 1) * UNIT_SOURCE.len();
    let cursor_offset = source[last_unit_start..]
        .find("s.tag")
        .map(|i| last_unit_start + i)
        .expect("bench fixture carries the `s.tag` read");

    // Policy bench: the choice over the shared index through the seam,
    // no database locks — the unit the policy tests pin. The index guard
    // lives in this block only, so the wire bench below starts from a
    // lock-free database.
    {
        let shared_index = db.symbol_index.read();
        let source_impl = SymbolIndexSource::new(&shared_index);
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let target = type_target(&source_impl, "Shape");
            hits += usize::from(target.is_some());
            black_box(&target);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {hits} hits)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (offset → typeck → peel → choice →
    // location mapping).
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&path)
                    .ok()
                    .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                    .expect("bench fixture path converts to a URI"),
            },
            position: {
                let mut line = 0u32;
                let mut last_nl = 0usize;
                for (i, ch) in source.char_indices() {
                    if i >= cursor_offset {
                        break;
                    }
                    if ch == '\n' {
                        line += 1;
                        last_nl = i + 1;
                    }
                }
                Position {
                    line,
                    character: source[last_nl..cursor_offset]
                        .chars()
                        .map(char::len_utf16)
                        .sum::<usize>() as u32,
                }
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let response = type_definition(&db, &file_map, &params).expect("request hits the shape");
        wire_hits += 1;
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_hits} resolved)",
        wire / 1_000
    );

    black_box(wire_hits);
}
