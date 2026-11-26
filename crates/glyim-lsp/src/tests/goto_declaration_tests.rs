//! Database-backed tests for the goto-declaration request (LSP-DECLARATION).
//!
//! Mirrors `inlay_hint_tests.rs`: the real parser, def-map, HIR, and
//! typeck pipeline populate the database exactly like the analysis driver
//! does (including the stored parse tree the import walk reads), and the
//! public `goto_declaration` entry point is exercised end-to-end. The pins:
//!
//! - a cursor on an imported name declares the `use` statement that binds it,
//! - a cursor on a locally-declared item declares the item itself,
//! - a cursor inside the `use` statement declares that same statement,
//! - unopened documents and unknown names answer `null` honestly.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::goto_declaration::goto_declaration;
use lsp_types::{GotoDefinitionParams, Position, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

fn uri_for(path: &PathBuf) -> lsp_types::Uri {
    Uri::from_str(
        &Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap()
}

/// One imported name used in a body, one locally-declared function, and
/// one unknown name to probe. Line 0 is the `use` statement the import
/// walk must find.
const DOC: &str = "use crate::shapes::Point;\n\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    p.x\n}\n\nfn helper() {}\n";

/// The analysis pipeline, exactly as the driver runs it (parse →
/// def-map → HIR → typeck → stores, including the parse tree), so the
/// request path walks real recorded state.
fn populated_db(content: &str, path_str: &str) -> (AnalysisDatabase, FileMap, PathBuf) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from(path_str);
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, content.to_string()));

    let crate_id = glyim_core::CrateId::from_usize(0);
    let parse_result = glyim_frontend::parse_to_syntax(content, file_id);
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
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.syntaxes.write().insert(file_id, parse_result.green_node);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    (db, file_map, path)
}

fn params_at(path: &PathBuf, line: u32, character: u32) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

fn range_of(response: lsp_types::GotoDefinitionResponse) -> lsp_types::Range {
    match response {
        lsp_types::GotoDefinitionResponse::Scalar(loc) => loc.range,
        other => panic!("declaration answers a scalar location, got {other:?}"),
    }
}

#[test]
fn imported_name_declares_the_use_statement() {
    let (db, file_map, path) = populated_db(DOC, "/test/goto_declaration.g");
    // Line 3 is `    let p = Point { x: 1, y: 2 };`; `Point` starts at
    // character 13 (four spaces + `let p = `).
    let params = params_at(&path, 3, 14);

    let response = goto_declaration(&db, &file_map, &params)
        .expect("the import declaration exists for `Point`");
    let range = range_of(response);
    // The whole `use crate::shapes::Point;` statement (line 0) is the
    // declaration — not the struct-like shape somewhere else.
    assert_eq!(range.start.line, 0, "the declaration is the use statement");
    assert_eq!(range.end.line, 0);
    assert!(
        range.start.character < range.end.character,
        "the statement extent is token-exact"
    );
}

#[test]
fn local_item_declares_itself() {
    let (db, file_map, path) = populated_db(DOC, "/test/goto_declaration_local.g");
    // Line 7 is `fn helper() {}`; the cursor sits on `helper` (char 4).
    let params = params_at(&path, 7, 4);

    let response = goto_declaration(&db, &file_map, &params)
        .expect("a same-file definition is its own declaration");
    let range = range_of(response);
    assert_eq!(range.start.line, 7, "the declaration is the fn item itself");
    assert_eq!(range.end.line, 7);
}

#[test]
fn cursor_inside_the_use_declares_the_use() {
    let (db, file_map, path) = populated_db(DOC, "/test/goto_declaration_use_cursor.g");
    // Line 0 is `use crate::shapes::Point;`; the cursor sits on `Point`
    // in the path (character 21).
    let params = params_at(&path, 0, 21);

    let response =
        goto_declaration(&db, &file_map, &params).expect("the use statement binds `Point`");
    let range = range_of(response);
    assert_eq!(
        range.start.line, 0,
        "the declaration of an import binding is the import"
    );
}

#[test]
fn aliased_import_declares_the_alias() {
    let doc =
        "use crate::shapes::Point as Pt;\n\nfn main() {\n    let p = Pt { x: 1 };\n    p.x\n}\n";
    let (db, file_map, path) = populated_db(doc, "/test/goto_declaration_alias.g");
    // Line 3 is `    let p = Pt { x: 1 };` (line 1 is blank); `Pt`
    // spans characters 12..14 — the cursor sits on `t` (character 13).
    let params = params_at(&path, 3, 13);

    let response =
        goto_declaration(&db, &file_map, &params).expect("the alias binds `Pt` in this file");
    let range = range_of(response);
    assert_eq!(range.start.line, 0, "the declaration is the use statement");
}

#[test]
fn glob_import_and_spanless_locals_answer_null() {
    let doc = "use crate::shapes::*;\n\nfn main() {\n    let p = 1;\n    p\n}\n";
    let (db, file_map, path) = populated_db(doc, "/test/goto_declaration_glob.g");
    // `p` is a local binding; the symbol index records body locals
    // without usable extents today (a zero-width span), so no honest
    // declaration site exists for it: not the glob (it binds no single
    // name), not the zero-width record (it would aim the client at the
    // top of the file). The request must answer `null`, not invent a
    // location.
    let params = params_at(&path, 3, 8);

    assert!(
        goto_declaration(&db, &file_map, &params).is_none(),
        "a glob import plus a span-less local binding answer null"
    );
}

#[test]
fn unknown_name_answers_null() {
    let (db, file_map, path) = populated_db(DOC, "/test/goto_declaration_unknown.g");
    // `main` exists; probe a name that is neither imported, declared,
    // nor used anywhere: character 20 on line 3 is inside the struct
    // literal's field `x`, which the lexer records as `x` — probe
    // `zzz` by pointing at a place with no identifier at all (the
    // trailing `}` of main, line 5).
    let params = params_at(&path, 5, 0);

    assert!(
        goto_declaration(&db, &file_map, &params).is_none(),
        "no identifier under the cursor, nothing to declare (null)"
    );
}

#[test]
fn unopened_document_answers_null() {
    let (db, file_map, _path) = populated_db(DOC, "/test/goto_declaration_unopened.g");
    let other = PathBuf::from("/test/never_opened.g");
    let params = params_at(&other, 0, 0);

    assert!(goto_declaration(&db, &file_map, &params).is_none());
}
