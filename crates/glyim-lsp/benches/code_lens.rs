//! Code-lens request bench (LSP-CODE-LENS).
//!
//! Dependency-free (`harness = false`), mirroring the `document_link`
//! bench: measures the code-lens request path end-to-end over a synthetic
//! item-heavy corpus — the policy (item-level definitions + name-keyed
//! use counts through the `LensSource` seam over the shared reference
//! graph) and the full wire requests (`textDocument/codeLens` URI →
//! open-file lookup → graph walk → wire mapping, and the
//! `codeLens/resolve` echo re-join) — so CI logs can track the
//! per-request cost as the lens seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench code_lens
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_core::{CrateId, Interner};
use glyim_def_map::build_def_map;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_lsp::code_lens::{code_lenses, resolve_code_lens};
use glyim_lsp::database::{FileMap, SourceMap};
use lsp_types::{
    CodeLensParams, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
};
use url::Url;

/// One unit: two functions (one called once, one uncalled) and a struct —
/// the realistic mix an editor session shows (some items referenced, some
/// not). `helper` is called once per unit, so its use count scales with
/// the corpus while the lens list scales with the item count.
const UNIT_SOURCE: &str = "fn unit() -> i32 {\n\
                           helper(0)\n\
                           }\n\
                           fn helper(x: i32) -> i32 {\n\
                           x\n\
                           }\n\
                           struct Rec {\n\
                           a: i32,\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` item groups, analysed once through the
    // same pipeline the analysis driver runs (parse → def-map → HIR),
    // the reference graph populated exactly like the driver populates it.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/code_lens.gly");
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
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    let item_defs = db
        .reference_graph
        .read()
        .iter_with_names()
        .filter(|(_, r)| r.is_definition && r.is_item_level && !r.span.is_dummy())
        .count();
    assert_eq!(item_defs, UNITS * 3, "two fns + one struct per unit");

    // Policy bench: the lens list over the stored graph through the seam,
    // no wire mapping — the unit the policy tests pin. The graph guard
    // lives in this block only, so the wire bench below starts from a
    // lock-free database.
    let t0 = Instant::now();
    let mut lens_hits = 0usize;
    {
        let graph = db.reference_graph.read();
        let source = glyim_lsp::code_lens::GraphLensSource::new(&graph, file_id);
        for _ in 0..1_000 {
            let lenses = glyim_lsp::code_lens::compute_code_lenses(&source);
            lens_hits += lenses.len();
            black_box(&lenses);
        }
    }
    let policy = t0.elapsed();
    println!(
        "policy compute : {:>9.1?}/call (1,000 calls, {lens_hits} lenses total, {item_defs} item defs)",
        policy / 1_000
    );

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (URI → open-file lookup → graph
    // walk → wire mapping).
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("bench fixture path converts to a URI");
    let params = CodeLensParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let response = code_lenses(&db, &file_map, &params).expect("request finds lenses");
        wire_hits += response.len();
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_hits} lenses mapped)",
        wire / 1_000
    );

    // Resolve bench: the echoed first lens re-joined against the graph —
    // the per-render cost the editor pays on top of the enumeration.
    let first = code_lenses(&db, &file_map, &params).expect("request finds lenses");
    let echoed = first
        .first()
        .expect("corpus carries item-level defs")
        .clone();
    let t2 = Instant::now();
    for _ in 0..1_000 {
        let resolved = resolve_code_lens(&db, &file_map, &echoed).expect("echo resolves");
        black_box(&resolved);
    }
    let resolve = t2.elapsed();
    println!(
        "wire resolve   : {:>9.1?}/call (1,000 calls)",
        resolve / 1_000
    );

    black_box(wire_hits);
}
