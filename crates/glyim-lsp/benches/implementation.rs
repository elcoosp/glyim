//! Implementation-request bench (LSP-IMPLEMENTATION).
//!
//! Dependency-free (`harness = false`), mirroring the `type_definition`
//! and `goto_declaration` benches: measures the implementation request
//! path end-to-end over a synthetic typed corpus — the choice policy
//! (impl-site selection through the `ImplSource` seam over the stored
//! HIRs) and the full wire request (offset → identifier → multi-file HIR
//! walk → location mapping) — so CI logs can track the per-request cost
//! as the HIR impl seam grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench implementation
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::implementation::{HirImplSource, goto_implementation, implementation_targets};
use lsp_types::{
    GotoDefinitionParams, PartialResultParams, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use url::Url;

/// One unit: a trait, a struct, a trait impl, and an inherent impl —
/// enough for the seam to carry several impl sites the choice policy must
/// sift per request.
const UNIT_SOURCE: &str = "trait Shape {\n\
                           fn area(&self) -> i32;\n\
                           }\n\
                           struct Unit { w: i32 }\n\
                           impl Shape for Unit {\n\
                           fn area(&self) -> i32 { self.w }\n\
                           }\n\
                           impl Unit {\n\
                           fn double(&self) -> i32 { self.w * 2 }\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` trait/impl groups, lowered once, the
    // database populated exactly like the analysis driver does.
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/implementation.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    // The driver's parse → HIR pipeline: the implementation engine reads
    // recorded HIR facts only, so no typeck pass is on its path.
    let parse_result = glyim_frontend::parse_to_syntax(&source, file_id);
    let mut interner = glyim_core::Interner::new();
    let (hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
    db.hirs.write().insert(file_id, hir);

    // Cursor on the trait name inside the LAST unit's impl head, so the
    // query has the full corpus of impl sites behind it.
    let last_unit_start = (UNITS - 1) * UNIT_SOURCE.len();
    let cursor_offset = last_unit_start
        + UNIT_SOURCE
            .find("impl Shape for Unit")
            .map(|i| i + "impl ".len())
            .expect("bench fixture carries the `impl Shape for Unit` head");

    // Policy bench: the choice over the session's stored HIRs through the
    // seam, no database locks — the unit the policy tests pin. The HIR
    // guard lives in this block only, so the wire bench below starts from
    // a lock-free database.
    {
        let hirs = db.hirs.read();
        let files: Vec<(glyim_span::FileId, &glyim_hir::CrateHir)> =
            hirs.iter().map(|(id, hir)| (*id, hir)).collect();
        let source_impl = HirImplSource::new(files);
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let targets = implementation_targets(&source_impl, "Shape");
            hits += targets.len();
            black_box(&targets);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {hits} targets total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (offset → identifier → multi-file
    // HIR walk → choice → location mapping).
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
        let response =
            goto_implementation(&db, &file_map, &params).expect("request hits the trait");
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
