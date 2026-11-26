//! Database-backed tests for the type-definition request (LSP-TYPE-DEF).
//!
//! Mirrors `tier6_4_completion_tests.rs`: the real parser, def-map, HIR,
//! and typeck pipeline populate the database exactly like the analysis
//! driver does, and the public `type_definition` entry point is exercised
//! end-to-end (cursor resolution → expression type → `Ty` peeling →
//! symbol choice → wire location). This is also the regression pin for
//! the TCK-SEAM fix: `type_at_offset` must translate HIR `BodyId`s
//! through `TypeckResult::body_owners`, because the HIR and typeck
//! `LocalDefId` counters are independent — before the fix, every query
//! resolved the wrong body and the response was always `null`.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::type_definition::type_definition;
use glyim_span::FileId;
use lsp_types::{GotoDefinitionResponse, Position, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(
        &Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap()
}

/// One struct, one let whose initializer constructs it, and a tail
/// expression reading a field through the binding.
const DOC: &str =
    "struct Point { x: i32, y: i32 }\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    p.y\n}\n";

/// Byte offsets of the tokens the cursors sit on (verified against DOC).
const POINT_CONSTRUCTOR: usize = 56; // `Point` in the initializer
const P_BINDING: usize = 52; // `p` in `let p`
const P_FIELD_READ: usize = 82; // `p` in `p.y`
const INT_LITERAL: usize = 67; // `1` in the initializer

fn populated_db() -> (AnalysisDatabase, FileMap, PathBuf, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/type_def.g");
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline (parse → def-map → HIR → typeck →
    // symbol index), so the request path walks real typeck state.
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

    (db, file_map, path, file_id)
}

fn params_at(path: &PathBuf, offset: usize) -> lsp_types::GotoDefinitionParams {
    let (line, character) = {
        let mut line = 0u32;
        let mut last_nl = 0usize;
        for (i, ch) in DOC.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                last_nl = i + 1;
            }
        }
        (
            line,
            DOC[last_nl..offset]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        )
    };
    lsp_types::GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

/// The `Point` definition range the response must carry. The symbol index
/// records the whole struct item as the definition site (`struct Point {
/// x: i32, y: i32 }` = bytes 0..31 on line 0), the same extent
/// goto-definition answers with for the same symbol.
const POINT_DEF: (usize, usize) = (0, 31);

fn expect_point_location(response: GotoDefinitionResponse) {
    match response {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.range.start.line, 0);
            assert_eq!(location.range.start.character as usize, POINT_DEF.0);
            assert_eq!(location.range.end.character as usize, POINT_DEF.1);
        }
        other => panic!("expected a scalar location, got {other:?}"),
    }
}

#[test]
fn type_definition_jumps_from_a_value_to_its_struct() {
    let (db, file_map, path, _file_id) = populated_db();
    let params = params_at(&path, P_FIELD_READ);

    let response = type_definition(&db, &file_map, &params).expect("p carries the Point shape");

    expect_point_location(response);
}

#[test]
fn type_definition_works_from_the_constructor_too() {
    let (db, file_map, path, _file_id) = populated_db();
    let params = params_at(&path, POINT_CONSTRUCTOR);

    let response =
        type_definition(&db, &file_map, &params).expect("the constructor carries the shape");

    expect_point_location(response);
}

#[test]
fn type_definition_none_on_the_pattern_position() {
    // A cursor parked on the binding *name* (`let p`) sits on a pattern,
    // not an expression — the type checker records types for
    // expressions, so there is nothing to resolve there. The value *uses*
    // (`p.y`) and the constructor expression are the supported entry
    // points; answering `null` here is honest rather than guessing.
    let (db, file_map, path, _file_id) = populated_db();
    let params = params_at(&path, P_BINDING);

    assert!(type_definition(&db, &file_map, &params).is_none());
}

#[test]
fn type_definition_none_for_primitives() {
    let (db, file_map, path, _file_id) = populated_db();
    let params = params_at(&path, INT_LITERAL);

    assert!(
        type_definition(&db, &file_map, &params).is_none(),
        "i32 has no user-defined definition site"
    );
}

#[test]
fn type_definition_none_for_unknown_document() {
    let (db, file_map, _path, _file_id) = populated_db();
    let closed = PathBuf::from("/test/never-opened.g");
    let params = params_at(&closed, P_FIELD_READ);

    assert!(type_definition(&db, &file_map, &params).is_none());
}
