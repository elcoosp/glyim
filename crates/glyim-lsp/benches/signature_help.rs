//! Signature-help bench (LSP-SIGNATURE-HELP).
//!
//! Dependency-free (`harness = false`), mirroring the `call_hierarchy`
//! and `type_definition` benches: measures the signature-help request
//! path end-to-end over a synthetic corpus — the policy computation
//! through the `CallSiteSource`/`SignatureSource` seams and the full wire
//! request exactly as the router handler drives it — so CI logs can
//! track the per-request cost as the seam grows consumers. The
//! `call_site_at` path deliberately includes the innermost-scan over
//! `iter_with_names`, because its linear cost is what a future
//! span-indexed call cache would buy back. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench signature_help
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::signature_help::{
    ReferenceGraphCallSiteSource, SignatureIndexSource, compute_signature_help, signature_help,
};
use lsp_types::{
    SignatureHelpParams, TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
};
use url::Url;

/// One call-graph unit: a two-parameter callee invoked by a caller —
/// enough for the innermost scan to sift same-shape records.
const UNIT_SOURCE: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\
fn entry() -> i32 {\n    add(1, 2)\n}\n";
const UNITS: usize = 32;

fn main() {
    // Build the corpus: `units` call units, analyzed once, the database
    // populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/signature_help.g");
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
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);

    // Cursor on the FIRST unit's `2` argument — the first `add` is the
    // signature the index answers, while the joins still scan the full
    // corpus (every unit records an `add` call).
    let second_arg_offset = source
        .find("add(1, 2)")
        .map(|i| i + 7)
        .expect("bench fixture carries `add(1, 2)`");

    // Policy bench: the innermost-call scan + signature lookup + active
    // parameter count through the seams, no database locks. The guards
    // live in this block only, so the wire bench below starts from a
    // lock-free database.
    {
        let graph = db.reference_graph.read();
        let index = db.symbol_index.read();
        let call_sites = ReferenceGraphCallSiteSource::new(&graph);
        let signatures = SignatureIndexSource::new(&index);
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let plan = compute_signature_help(
                &call_sites,
                &signatures,
                &source,
                file_id,
                second_arg_offset,
            );
            hits += usize::from(plan.is_some());
            black_box(&plan);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute       : {:>9.1?}/call (1,000 calls, {hits} hits)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it.
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("bench fixture path converts to a URI");
    let (line, character) = {
        let mut line = 0u32;
        let mut last_nl = 0usize;
        for (i, ch) in source.char_indices() {
            if i >= second_arg_offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                last_nl = i + 1;
            }
        }
        (
            line,
            source[last_nl..second_arg_offset]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        )
    };
    let params = SignatureHelpParams {
        context: None,
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: lsp_types::Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let help = signature_help(&db, &file_map, &params).expect("request hits the call");
        wire_hits += 1;
        black_box(&help);
    }
    let wire = t1.elapsed();
    println!(
        "wire request         : {:>9.1?}/call (1,000 calls, {wire_hits} resolved)",
        wire / 1_000
    );

    black_box(wire_hits);
}
