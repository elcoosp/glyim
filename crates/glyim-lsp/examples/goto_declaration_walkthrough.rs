//! Consumer walkthrough of the goto-declaration seam (LSP-DECLARATION):
//! the `textDocument/declaration` capability behind an editor's
//! "go to declaration" command.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example goto_declaration_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document runs through the same parse → def-map → HIR →
//! typeck pipeline the analysis driver runs, the results are stored in
//! `AnalysisDatabase` exactly like the driver stores them (including the
//! parse tree the import walk reads), and `goto_declaration` — the
//! function the router registers for `GotoDeclaration` — answers where
//! the cursor's name is *declared for this file*. An import declaration
//! that binds the name wins over a same-named local item (that is what
//! the user wrote to bring the name into scope), a locally-declared item
//! declares itself, and a name that is neither imported nor declared in
//! the file answers `null` honestly. Every offset is derived from the
//! document by token search, so the walkthrough cannot silently drift
//! from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::goto_declaration::goto_declaration;
use lsp_types::{
    GotoDefinitionParams, Position, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

/// The walkthrough document: one import (with an alias, the shape the
/// declaration request is most useful on), the local struct the import
/// resolves to, a body use of the alias, and a locally-declared function.
const DOC: &str = "use crate::shapes::Point as Pt;\nstruct Point { x: i32, y: i32 }\nfn area(p: Pt) -> i32 {\n    p.x\n}\nfn main() {\n    let p = Pt { x: 1, y: 2 };\n    area(p)\n}\n";

/// Byte offset of the first `needle` in DOC, advanced `into` bytes so
/// the cursor lands on the identifier at the end of the needle (an
/// editor sends the position of the name, not the start of the line).
fn offset_into(needle: &str, into: usize) -> usize {
    let base = DOC
        .find(needle)
        .unwrap_or_else(|| panic!("walkthrough fixture drifted: `{needle}` not found"));
    base + into
}

/// `(line, utf16 column)` of byte `offset`, the position an editor
/// would send for a cursor there.
fn position_at(offset: usize) -> Position {
    let before = &DOC[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Position {
        line,
        character: (offset - line_start) as u32,
    }
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/goto_declaration.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline, so the request path walks real
    // recorded state (the parse tree carries the `use` declarations).
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
    db.syntaxes.write().insert(file_id, parse_result.green_node);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    let request = |label: &str, at: Position| {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: lsp_types::Uri::from_str(Url::from_file_path(&path).unwrap().as_str())
                        .unwrap(),
                },
                position: at,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        };
        let response = goto_declaration(&db, &file_map, &params);
        println!("{label}");
        match response {
            Some(lsp_types::GotoDefinitionResponse::Scalar(loc)) => {
                println!(
                    "  -> declared at line {} (chars {}..{})",
                    loc.range.start.line, loc.range.start.character, loc.range.end.character
                );
            }
            Some(other) => println!("  -> unexpected response shape: {other:?}"),
            None => println!("  -> null (no local declaration to jump to)"),
        }
    };

    // 1. Cursor on a use of the imported alias in a body: the
    //    declaration is the `use` statement, not the struct definition.
    request(
        "1. cursor on the alias use (`let p = Pt`) — the import declares it:",
        position_at(offset_into("let p = Pt", "let p = ".len())),
    );
    // 2. Cursor inside the `use` statement itself: it is its own
    //    declaration.
    request(
        "2. cursor on the alias in the import (`as Pt`) — the import declares itself:",
        position_at(offset_into("as Pt", "as ".len())),
    );
    // 3. Cursor on a locally-declared function: the item is its own
    //    declaration (the spec's same-file case).
    request(
        "3. cursor on the local `fn area` — the item declares itself:",
        position_at(offset_into("fn area", "fn ".len())),
    );
}
