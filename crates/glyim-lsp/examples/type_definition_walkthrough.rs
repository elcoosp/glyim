//! Consumer walkthrough of the type-definition seam (LSP-TYPE-DEF): the
//! `textDocument/typeDefinition` capability behind an editor's "go to
//! type definition".
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example type_definition_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR →
//! typeck pipeline the analysis driver runs, the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them, and
//! `type_definition` — the function the router registers for
//! `GotoTypeDefinition` — turns a cursor on a *value* into the location
//! of the struct its type is defined at. A primitive-typed cursor
//! answers `null` honestly: there is no user-defined site to jump to.
//! Every offset is derived from the document by token search, so the
//! walkthrough cannot silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::type_definition::type_definition;
use lsp_types::{
    GotoDefinitionParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

/// The walkthrough document: one struct, a binding constructed from it,
/// a field read through the binding, and an integer literal.
const DOC: &str =
    "struct Point { x: i32, y: i32 }\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    p.y\n}\n";

/// Byte offset of the first `needle` in DOC.
fn find(needle: &str) -> usize {
    DOC.find(needle)
        .unwrap_or_else(|| panic!("walkthrough fixture drifted: `{needle}` not found"))
}

/// Byte offset -> (line, utf16 character); the inverse of
/// `SourceMap::line_col_to_offset`, derived from `DOC` so the requests
/// cannot lie about columns.
fn line_col_of(offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, ch) in DOC.char_indices() {
        if i == offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    let character = DOC[last_nl..offset]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>();
    (line, character as u32)
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/type_def.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline, so the request path walks real
    // typeck state (this is what makes `type_at_offset` work).
    let crate_id = glyim_core::CrateId::from_raw(0);
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

    // Cursor on the field read (`p.y`) and on the integer literal.
    let field_read_offset = find("p.y");
    let literal_offset = find("x: 1") + 3;
    let params_for = |offsets: &[usize]| GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&path)
                    .ok()
                    .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                    .expect("walkthrough path converts to a URI"),
            },
            position: {
                let (line, character) = line_col_of(offsets[0]);
                Position { line, character }
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    println!("document: {DOC:?}\n");

    let params = params_for(&[field_read_offset]);
    match type_definition(&db, &file_map, &params) {
        Some(lsp_types::GotoDefinitionResponse::Scalar(location)) => {
            let r = &location.range;
            println!(
                "typeDefinition on `p.y` (byte {field_read_offset}) → struct site at {}:{} .. {}:{}\n",
                r.start.line, r.start.character, r.end.line, r.end.character,
            );
        }
        Some(other) => println!("typeDefinition on `p.y` → unexpected response {other:?}\n"),
        None => println!("typeDefinition on `p.y` → null (unexpected)\n"),
    }

    let params = params_for(&[literal_offset]);
    match type_definition(&db, &file_map, &params) {
        Some(_) => println!("typeDefinition on the literal → unexpected non-null"),
        None => println!(
            "typeDefinition on the integer literal (byte {literal_offset}) → null \
             (primitives have no user-defined site)"
        ),
    }
}
