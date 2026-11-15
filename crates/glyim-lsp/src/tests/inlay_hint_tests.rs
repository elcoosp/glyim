//! Database-backed tests for the inlay-hint request (LSP-INLAY-HINT).
//!
//! Mirrors `type_definition_tests.rs`: the real parser, def-map, HIR,
//! and typeck pipeline populate the database exactly like the analysis
//! driver does, and the public `inlay_hints` entry point is exercised
//! end-to-end (HIR `let` walk → initializer type → label render → wire
//! positions). The pins:
//!
//! - un-annotated bindings hint `: Ty` at the end of the binding name,
//! - annotated bindings hint nothing (the source already says it),
//! - the requested screen range restricts the answer,
//! - unopened documents and binding-free files answer `null` honestly.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::inlay_hint::inlay_hints;
use lsp_types::{
    InlayHintKind, InlayHintLabel, InlayHintParams, Position, Range, TextDocumentIdentifier, Uri,
};
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

/// One struct, an un-annotated binding constructed from it, an
/// annotated binding, a second un-annotated primitive binding, and a
/// tail expression using all three.
const DOC: &str = "struct Point { x: i32, y: i32 }\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    let annotated: i32 = 7;\n    let n = 2 + 3;\n    p.y + annotated + n\n}\n";

/// Expected hint attachments (0-based line / utf16 character): the
/// character right after the binding name.
const P_HINT: (usize, usize) = (2, 9); // after `p` in `let p`
const N_HINT: (usize, usize) = (4, 9); // after `n` in `let n`

/// The analysis pipeline, exactly as the driver runs it (parse →
/// def-map → HIR → typeck → stores), so the request path walks real
/// typeck state.
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

    let crate_id = glyim_core::CrateId::from_raw(0);
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
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    (db, file_map, path)
}

fn params_for(path: &PathBuf, range: Range) -> InlayHintParams {
    InlayHintParams {
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        text_document: TextDocumentIdentifier { uri: uri_for(path) },
        range,
    }
}

fn whole_document() -> Range {
    // One line past the last content line: the SourceMap maps it to
    // EOF, and the range filter then covers every attachment. (An end
    // line far past EOF would be unmappable and answer `null` — the
    // honest response for a range that names no real lines.)
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: DOC.lines().count() as u32,
            character: 0,
        },
    }
}

#[test]
fn unannotated_bindings_get_type_hints() {
    let (db, file_map, path) = populated_db(DOC, "/test/inlay_hint.g");
    let params = params_for(&path, whole_document());

    let hints = inlay_hints(&db, &file_map, &params).expect("bindings exist, hints must exist");
    assert_eq!(hints.len(), 2, "p and n hint; annotated does not");

    let first_label = label_of(&hints[0].label);
    assert_eq!(
        hints[0].position,
        Position {
            line: P_HINT.0 as u32,
            character: P_HINT.1 as u32,
        },
        "hint attaches after the binding name"
    );
    assert_eq!(
        first_label, ": Point",
        "the constructor's shape is the binding's type"
    );
    assert_eq!(hints[0].kind, Some(InlayHintKind::TYPE));

    let second = &hints[1];
    assert_eq!(
        second.position,
        Position {
            line: N_HINT.0 as u32,
            character: N_HINT.1 as u32,
        },
    );
    assert_eq!(label_of(&second.label), ": i32");
    assert_eq!(second.kind, Some(InlayHintKind::TYPE));
    // Display-only: the engine never proposes an edit the user did not
    // ask for.
    assert!(second.text_edits.is_none());
}

/// `InlayHintLabel` carries no `PartialEq` (lsp-types 0.97), so tests
/// destructure the string form.
fn label_of(label: &InlayHintLabel) -> &str {
    match label {
        InlayHintLabel::String(s) => s,
        other => panic!("engine emits string labels, got {other:?}"),
    }
}

#[test]
fn annotated_bindings_get_no_hint_on_their_line() {
    let (db, file_map, path) = populated_db(DOC, "/test/inlay_hint_annotated.g");
    let params = params_for(&path, whole_document());

    let hints = inlay_hints(&db, &file_map, &params).expect("hints exist");
    // Line 3 is `let annotated: i32 = 7;` — the annotation already
    // says the type, so nothing attaches there.
    let on_annotation_line = hints.iter().filter(|h| h.position.line == 3).count();
    assert_eq!(on_annotation_line, 0, "an annotated binding never hints");
}

#[test]
fn requested_range_restricts_hints() {
    let (db, file_map, path) = populated_db(DOC, "/test/inlay_hint_range.g");
    // Line 4 only: `let n = 2 + 3;`.
    let params = params_for(
        &path,
        Range {
            start: Position {
                line: 4,
                character: 0,
            },
            end: Position {
                line: 5,
                character: 0,
            },
        },
    );

    let hints = inlay_hints(&db, &file_map, &params).expect("one hint lives on line 4");
    assert_eq!(hints.len(), 1, "p's hint (line 2) is outside the range");
    assert_eq!(hints[0].position.line, N_HINT.0 as u32);
}

#[test]
fn unopened_document_answers_null() {
    let (db, file_map, _path) = populated_db(DOC, "/test/inlay_hint_unopened.g");
    let other = PathBuf::from("/test/never_opened.g");
    let params = params_for(&other, whole_document());

    assert!(inlay_hints(&db, &file_map, &params).is_none());
}

#[test]
fn file_without_bindings_answers_null() {
    let (db, file_map, path) = populated_db("fn main() {\n}\n", "/test/inlay_hint_empty.g");
    let params = params_for(&path, whole_document());

    assert!(
        inlay_hints(&db, &file_map, &params).is_none(),
        "no let bindings, nothing to answer (null, not an empty array)"
    );
}
