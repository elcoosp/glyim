//! Inlay-hint bench (LSP-INLAY-HINT).
//!
//! Dependency-free (`harness = false`), mirroring the `signature_help`
//! and `type_definition` benches: measures the inlay-hint request path
//! end-to-end over a synthetic corpus — the policy normalization through
//! the `BindingTypeSource` seam and the full wire request exactly as the
//! router handler drives it — so CI logs can track the per-request cost
//! as the seam grows consumers. The flat `exprs` walk is the whole
//! story today (per request: one pass over every HIR expression plus
//! one `SourceMap` position mapping per hint); the linear cost is what
//! a future let-index cache would buy back. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench inlay_hint
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::inlay_hint::{HirBindingTypeSource, compute_inlay_hints, inlay_hints};
use lsp_types::{InlayHintParams, Range, TextDocumentIdentifier, WorkDoneProgressParams};
use url::Url;

/// One analysis unit: two un-annotated bindings (one constructed shape,
/// one primitive) plus an annotated binding the filter must skip —
/// enough for the walk to sift annotated from un-annotated lets.
const UNIT_SOURCE: &str = "fn add(a: i32, b: i32) -> i32 {\n    let t = a + b;\n    let annotated: i32 = 7;\n    let u = t + annotated;\n    u\n}\n";
const UNITS: usize = 32;

fn main() {
    // Build the corpus: `units` analysis units, analyzed once, the
    // database populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/inlay_hint.g");
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
    let (_def_map, _def_diags) =
        glyim_def_map::build_def_map(&parse_result.root, crate_id, interner.clone());
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    let ty_ctx_mut = glyim_type::TyCtxMut::new(interner.clone());
    let trait_ctx = glyim_solve::TraitContext::new();
    let mut solver = glyim_solve::SimpleTraitSolver::new(&trait_ctx);
    let (ty_ctx, typeck_result) =
        glyim_typeck::typeck_crate(ty_ctx_mut, &_def_map, &hir, &mut solver);
    db.hirs.write().insert(file_id, hir);
    db.typeck
        .write()
        .insert(file_id, (std::sync::Arc::new(ty_ctx), typeck_result));

    // Policy bench: the HIR let walk + type rendering + normalization
    // through the seams, no database locks. The guards live in this
    // block only, so the wire bench below starts from a lock-free
    // database.
    {
        let hirs = db.hirs.read();
        let hir = hirs.get(&file_id).expect("corpus file is stored");
        let typeck_map = db.typeck.read();
        let (ty_ctx, typeck_result) = typeck_map.get(&file_id).expect("corpus typeck is stored");
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let hints = compute_inlay_hints(&HirBindingTypeSource::new(hir, typeck_result, ty_ctx));
            hits += hints.len();
            black_box(&hints);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute       : {:>9.1?}/call (1,000 calls, {hits} hints total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it. The whole document is the range
    // (one line past the last content line maps to EOF).
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("bench fixture path converts to a URI");
    let params = InlayHintParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        text_document: TextDocumentIdentifier { uri },
        range: Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: source.lines().count() as u32,
                character: 0,
            },
        },
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let hints =
            inlay_hints(&db, &file_map, &params).expect("the corpus carries un-annotated bindings");
        wire_hits += hints.len();
        black_box(&hints);
    }
    let wire = t1.elapsed();
    println!(
        "wire request         : {:>9.1?}/call (1,000 calls, {wire_hits} hints total)",
        wire / 1_000
    );

    black_box(wire_hits);
}
