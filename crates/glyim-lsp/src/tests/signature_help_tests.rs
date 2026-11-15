//! Database-backed tests for the signature-help request (LSP-SIGNATURE-HELP).
//!
//! Mirrors `type_definition_tests.rs` and `call_hierarchy_tests.rs`: the
//! real parser, def-map, HIR, and typeck pipeline populate the database
//! exactly like the analysis driver does, the shared `ReferenceGraph`
//! (call-site records) and `SymbolIndex` (function signatures) are built
//! from the real HIR, and the public `signature_help` entry point — the
//! function the router registers for `SignatureHelpRequest` — is
//! exercised end-to-end (cursor → innermost call site → callee signature
//! → active parameter → wire shape).

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::signature_help::signature_help;
use glyim_span::FileId;
use lsp_types::{Position, Uri};
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

/// One two-parameter function and a caller that invokes it twice — once
/// per argument position — plus an unresolved callee the honest-miss
/// test drives. Lines: 0-2 add · 3-6 main.
const DOC: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\
fn main() {\n    let s = add(1, 2);\n    let t = add(s, 4);\n    ghost(s)\n}\n";

/// Byte offsets the cursors sit on (verified against DOC).
const ADD_CALL_FIRST_ARG: usize = 72; // `1` in `add(1, 2)`
const ADD_CALL_SECOND_ARG: usize = 75; // `2` in `add(1, 2)`
const ADD_CALL_NAME: usize = 68; // `add` in `add(1, 2)`
const UNKNOWN_CALLEE: usize = 106; // `ghost` in `ghost(s)`
const OUTSIDE_ANY_CALL: usize = 0; // `fn` on line 0

fn populated_db() -> (AnalysisDatabase, FileMap, PathBuf, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/signature_help.g");
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline (parse → def-map → HIR → typeck →
    // symbol index → reference graph). The request path reads the
    // reference graph (call sites) and the symbol index (signatures);
    // the rest is populated to mirror the driver.
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
    db.reference_graph
        .write()
        .build_from_hir(file_id, &hir, &interner);
    db.hirs.write().insert(file_id, hir);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    (db, file_map, path, file_id)
}

fn position_of(offset: usize) -> Position {
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
    Position {
        line,
        character: DOC[last_nl..offset]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>() as u32,
    }
}

fn params_at(path: &PathBuf, offset: usize) -> lsp_types::SignatureHelpParams {
    lsp_types::SignatureHelpParams {
        context: None,
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
            position: position_of(offset),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    }
}

/// The rendered `add` signature the response must carry: parameters from
/// the symbol index's `TypeSignature`, rendered `fn add(a: i32, b: i32)
/// -> i32` — the same shape hover/completions render from.
fn expect_add_signature(help: lsp_types::SignatureHelp) {
    assert_eq!(help.signatures.len(), 1, "one resolved signature");
    assert_eq!(help.active_signature, Some(0), "single candidate");
    let sig = &help.signatures[0];
    assert_eq!(sig.label, "fn add(a: i32, b: i32) -> i32");
    let params = sig.parameters.as_ref().expect("parameter list");
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[0].label,
        lsp_types::ParameterLabel::Simple("a: i32".to_string())
    );
    assert_eq!(
        params[1].label,
        lsp_types::ParameterLabel::Simple("b: i32".to_string())
    );
}

#[test]
fn signature_help_resolves_the_call_and_its_active_parameter() {
    let (db, file_map, path, _file_id) = populated_db();

    let help = signature_help(&db, &file_map, &params_at(&path, ADD_CALL_SECOND_ARG))
        .expect("the cursor sits inside add(1, 2)");

    expect_add_signature(help.clone());
    // The `2` is add's second argument.
    assert_eq!(
        help.active_parameter,
        Some(1),
        "the cursor is on add's second argument"
    );
}

#[test]
fn the_first_argument_activates_parameter_zero() {
    let (db, file_map, path, _file_id) = populated_db();

    let help = signature_help(&db, &file_map, &params_at(&path, ADD_CALL_FIRST_ARG))
        .expect("the cursor sits inside add(1, 2)");

    assert_eq!(help.active_parameter, Some(0));
}

#[test]
fn the_callee_token_itself_resolves_the_call() {
    // Signature help fires on the callee name too — the editor's trigger
    // for typing `(` — not only on the arguments.
    let (db, file_map, path, _file_id) = populated_db();

    let help = signature_help(&db, &file_map, &params_at(&path, ADD_CALL_NAME))
        .expect("the cursor sits on the callee name");

    expect_add_signature(help.clone());
    assert_eq!(help.active_parameter, Some(0), "before any argument");
}

#[test]
fn the_second_call_site_resolves_independently() {
    // `add(s, 4)` — the second invocation activates parameter one when
    // the cursor is on `4`, proving the call-site lookup is positional,
    // not name-cached to the first call.
    let (db, file_map, path, _file_id) = populated_db();
    let cursor = DOC.find("add(s, 4)").expect("the second call") + 7; // the `4`

    let help = signature_help(&db, &file_map, &params_at(&path, cursor))
        .expect("the cursor sits inside add(s, 4)");

    expect_add_signature(help.clone());
    assert_eq!(help.active_parameter, Some(1));
}

#[test]
fn signature_help_none_for_an_unrecorded_callee() {
    // `ghost(s)` has no symbol-index entry — a function-typed local or
    // an unresolved name — so the request answers `null` rather than
    // fabricating a signature.
    let (db, file_map, path, _file_id) = populated_db();

    assert!(
        signature_help(&db, &file_map, &params_at(&path, UNKNOWN_CALLEE)).is_none(),
        "an unrecorded callee has no signature to show"
    );
}

#[test]
fn signature_help_none_off_any_call() {
    let (db, file_map, path, _file_id) = populated_db();

    assert!(
        signature_help(&db, &file_map, &params_at(&path, OUTSIDE_ANY_CALL)).is_none(),
        "a cursor outside every call has nothing to show"
    );
}
