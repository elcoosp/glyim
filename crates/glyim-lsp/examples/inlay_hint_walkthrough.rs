//! Consumer walkthrough of the inlay-hint seam (LSP-INLAY-HINT): the
//! `textDocument/inlayHint` capability behind an editor's inline type
//! annotations on un-annotated `let` bindings.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example inlay_hint_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR →
//! typeck pipeline the analysis driver runs, the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and
//! `inlay_hints` — the function the router registers for
//! `InlayHintRequest` — turns every un-annotated binding into an inline
//! `: Ty` hint at the end of its name. Annotated bindings hint nothing
//! (the source already says it), and a type with no stable user-facing
//! spelling (an inference variable, a closure) is skipped honestly. An
//! empty answer is `null`, matching the other request engines. Every
//! offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::inlay_hint::inlay_hints;
use lsp_types::{InlayHintParams, Range, TextDocumentIdentifier, WorkDoneProgressParams};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

/// The walkthrough document: one struct, two un-annotated bindings, one
/// annotated binding (which must not hint), and a tail expression using
/// all three.
const DOC: &str = "struct Point { x: i32, y: i32 }\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    let annotated: i32 = 7;\n    let n = 2 + 3;\n    p.y + annotated + n\n}\n";

/// Byte offset of the first `needle` in DOC.
fn find(needle: &str) -> usize {
    DOC.find(needle)
        .unwrap_or_else(|| panic!("walkthrough fixture drifted: `{needle}` not found"))
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/inlay_hint.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline, so the request path walks real
    // typeck state (this is what makes the binding types resolve).
    let crate_id = glyim_core::CrateId::from_usize(0);
    let parse_result = glyim_frontend::parse_to_syntax(DOC, file_id);
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

    // One whole-document request (the editor's startup shape) and one
    // range request covering only the last binding's line (the
    // editor's per-scroll shape). The end line one past the last
    // content line maps to EOF.
    let uri = Url::from_file_path(&path)
        .ok()
        .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
        .expect("walkthrough path converts to a URI");
    let params_for = |start_line: u32, end_line: u32| InlayHintParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: lsp_types::Position {
                line: start_line,
                character: 0,
            },
            end: lsp_types::Position {
                line: end_line,
                character: 0,
            },
        },
    };

    println!("document: {DOC:?}\n");

    let params = params_for(0, DOC.lines().count() as u32);
    match inlay_hints(&db, &file_map, &params) {
        Some(hints) => {
            println!("whole document → {} hints:", hints.len());
            for hint in &hints {
                let l = hint.position.line;
                let c = hint.position.character;
                let label = match &hint.label {
                    lsp_types::InlayHintLabel::String(s) => s.as_str(),
                    other => panic!("walkthrough emits string labels, got {other:?}"),
                };
                println!("  line {l}, col {c}{label}");
            }
            println!();
        }
        None => println!("whole document → null (unexpected)\n"),
    }

    let params = params_for(4, 5);
    match inlay_hints(&db, &file_map, &params) {
        Some(hints) => {
            let n_offset = find("let n =") + 4;
            println!(
                "range on `let n`'s line → {} hint at byte {n_offset} \
                 (the attachment is right after the binding name)",
                hints.len()
            );
        }
        None => println!("range on `let n`'s line → null (unexpected)"),
    }
}
