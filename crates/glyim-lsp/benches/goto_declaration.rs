//! Goto-declaration bench (LSP-DECLARATION).
//!
//! Dependency-free (`harness = false`), mirroring the `inlay_hint` and
//! `call_hierarchy` benches: measures the goto-declaration request path
//! end-to-end over a synthetic corpus — the policy choice through the
//! `DeclarationSource` seam and the full wire request exactly as the
//! router handler drives it — so CI logs can track the per-request cost
//! as the seam grows consumers. The import walk is a pass over the `use`
//! declarations of the stored parse tree plus one symbol-index lookup
//! and one `SourceMap` position mapping; the linear cost is what a
//! future import table would buy back. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench goto_declaration
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::goto_declaration::{
    ParseTreeDeclarationSource, declaration_target, goto_declaration,
};
use glyim_span::FileId;
use lsp_types::{
    GotoDefinitionParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};
use url::Url;

/// One analysis unit: a `use` declaration binding `Point`, the local
/// struct the name resolves to, a caller, and a body occurrence of the
/// imported name — enough for the walk to sift import declarations from
/// item definitions.
const UNIT_SOURCE: &str = "use crate::shapes::Point;\nstruct Point { x: i32, y: i32 }\nfn area(p: Point) -> i32 {\n    p.x\n}\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    area(p)\n}\n";
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
    let path = PathBuf::from("/bench/goto_declaration.g");
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
    db.symbol_index
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.syntaxes.write().insert(file_id, parse_result.green_node);
    db.typeck
        .write()
        .insert(file_id, (std::sync::Arc::new(ty_ctx), typeck_result));

    // Policy bench: the import walk over the stored parse tree + the
    // symbol-index choice through the seam, no wire mapping. The guards
    // live in this block only, so the wire bench below starts from a
    // lock-free database.
    {
        let syntaxes = db.syntaxes.read();
        let green = syntaxes.get(&file_id).expect("corpus file is stored");
        let index = db.symbol_index.read();
        let src = ParseTreeDeclarationSource::new(
            green,
            FileId::from_usize(file_id.to_raw() as usize),
            &index,
        );
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let Some(target) = declaration_target(&src, file_id, "Point") else {
                panic!("the corpus imports Point; the policy must answer");
            };
            hits += target.span.hi().to_usize() - target.span.lo().to_usize();
            black_box(&target);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute       : {:>9.1?}/call (1,000 calls, {hits} span bytes total)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it. The cursor sits on the imported
    // `Point` occurrence in the first unit's body (offsets derived from
    // the corpus, never hardcoded line math).
    let body_off = source
        .find("let p = Point")
        .map(|i| i + "let p = ".len())
        .expect("the corpus carries a body occurrence of the import");
    let before = &source[..body_off];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let params = {
        let uri = Url::from_file_path(&path)
            .ok()
            .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
            .expect("bench fixture path converts to a URI");
        GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line,
                    character: (body_off - line_start) as u32,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        }
    };
    let t1 = Instant::now();
    let mut wire_hits = 0usize;
    for _ in 0..1_000 {
        let Some(response) = goto_declaration(&db, &file_map, &params) else {
            panic!("the corpus imports Point; the request must answer");
        };
        if let lsp_types::GotoDefinitionResponse::Scalar(loc) = response {
            wire_hits += 1;
            black_box(&loc.range);
        }
    }
    let wire = t1.elapsed();
    println!(
        "wire request         : {:>9.1?}/call (1,000 calls, {wire_hits} resolved lines total)",
        wire / 1_000
    );

    black_box(wire_hits);
}
