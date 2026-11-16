//! Database-backed tests for the moniker request (LSP-MONIKER).
//!
//! Mirrors `goto_declaration_tests.rs`: the real parse → def-map → HIR →
//! typeck pipeline (the analysis driver's own lowering) populates the
//! database exactly like the driver does — including the parse tree and
//! the symbol index the moniker engine reads — and the public `monikers`
//! entry point is exercised end-to-end (URI → open-file lookup → cursor
//! offset → identifier extraction → cursor-position fact choice → wire
//! monikers).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use crate::moniker::monikers;
use glyim_span::FileId;
use lsp_types::request::MonikerRequest;
use lsp_types::{MonikerParams, Position, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

/// One imported name (plain and aliased shapes below), one locally
/// declared struct, one free function, one local binding, and one
/// unknown name to probe. Line 0 is the `use` statement the import walk
/// must find.
const DOC: &str = "use crate::shapes::Point;\n\
                   \n\
                   struct Widget { n: i32 }\n\
                   \n\
                   fn helper() -> i32 { 1 }\n\
                   \n\
                   fn main() -> i32 {\n\
                   \x20   let p = Point { x: 1, y: 2 };\n\
                   \x20   p.n\n\
                   }\n";

/// The aliased shape: the binding name differs from the path's last
/// segment; the moniker identifier is the *spelled path*, not the alias.
const DOC_ALIAS: &str = "use crate::shapes::Point as Pt;\n\
                         \n\
                         fn main() -> i32 {\n\
                         \x20   let q = Pt { x: 1 };\n\
                         \x20   q.x\n\
                         }\n";

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

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
    db.syntaxes.write().insert(file_id, parse_result.green_node);
    db.typeck
        .write()
        .insert(file_id, (Arc::new(ty_ctx), typeck_result));

    (db, file_map, path)
}

/// Params at a byte offset of `DOC` (the same line/UTF-16-column walk
/// the wire position mapping uses).
fn params_at(path: &PathBuf, source: &str, offset: usize) -> MonikerParams {
    let (line, character) = {
        let mut line = 0u32;
        let mut last_nl = 0usize;
        for (i, ch) in source.char_indices() {
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
            source[last_nl..offset]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>() as u32,
        )
    };
    MonikerParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

/// Byte offset of the first `needle` in `source`.
fn offset_of(source: &str, needle: &str) -> usize {
    source
        .find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

fn kind_of(m: &lsp_types::Moniker) -> Option<lsp_types::MonikerKind> {
    m.kind
}

/// A cursor parked on the binding inside the `use` statement answers the
/// import moniker with the exact path the source spells.
#[test]
fn cursor_on_the_use_statement_answers_the_spelled_import() {
    let (db, file_map, path) = populated_db(DOC, "/test/moniker_use.g");
    // The cursor sits on `Point` — the binding — not on a path segment.
    let params = params_at(&path, DOC, offset_of(DOC, "crate::shapes::Point") + 16);
    let mons = monikers(&db, &file_map, &params).expect("the import binding answers");
    assert_eq!(mons.len(), 1);
    assert_eq!(mons[0].scheme, "glyim");
    assert_eq!(mons[0].identifier, "crate::shapes::Point");
    assert_eq!(mons[0].unique, lsp_types::UniquenessLevel::Document);
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Import));
}

/// A bare usage of the imported name answers the import moniker too —
/// the usage refers to the imported symbol, and the spelled path is its
/// identity.
#[test]
fn a_usage_of_the_imported_name_answers_the_import() {
    let (db, file_map, path) = populated_db(DOC, "/test/moniker_usage.g");
    let params = params_at(&path, DOC, offset_of(DOC, "Point { x: 1, y: 2 }") + 2);
    let mons = monikers(&db, &file_map, &params).expect("the usage resolves to the import");
    assert_eq!(mons.len(), 1);
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Import));
    assert_eq!(mons[0].identifier, "crate::shapes::Point");
}

/// A cursor on a locally-declared item answers the export moniker with
/// the recorded qualified path (a crate-root item records its bare name).
#[test]
fn cursor_on_a_local_definition_answers_the_export() {
    let (db, file_map, path) = populated_db(DOC, "/test/moniker_export.g");
    let params = params_at(&path, DOC, offset_of(DOC, "struct Widget { n: i32 }") + 7);
    let mons = monikers(&db, &file_map, &params).expect("Widget is recorded");
    assert_eq!(mons.len(), 1);
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Export));
    assert_eq!(mons[0].identifier, "Widget");

    let params = params_at(&path, DOC, offset_of(DOC, "fn helper() -> i32") + 3);
    let mons = monikers(&db, &file_map, &params).expect("helper is recorded");
    assert_eq!(mons.len(), 1);
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Export));
    assert_eq!(mons[0].identifier, "helper");
}

/// A local binding the index records without a usable extent answers
/// nothing — no identity is guessed for a zero-width recording, and the
/// file binds no import under the binding's own name.
#[test]
fn a_local_binding_answers_null() {
    let (db, file_map, path) = populated_db(DOC, "/test/moniker_local.g");
    let params = params_at(&path, DOC, offset_of(DOC, "let p = Point") + 4);
    assert!(monikers(&db, &file_map, &params).is_none());
}

/// A name nothing records answers `null` — never a fabricated identity.
/// The cursor probes a field name (`n` of `Widget`): fields are not
/// recorded as symbols, so no fact identifies them.
#[test]
fn an_unknown_name_answers_null() {
    let (db, file_map, path) = populated_db(DOC, "/test/moniker_unknown.g");
    let params = params_at(&path, DOC, offset_of(DOC, "p.n") + 2);
    assert!(monikers(&db, &file_map, &params).is_none());
}

/// The aliased import: the cursor on the alias binds through the spelled
/// path — the identifier is `crate::shapes::Point`, not `Pt`.
#[test]
fn an_aliased_import_answers_the_spelled_path() {
    let (db, file_map, path) = populated_db(DOC_ALIAS, "/test/moniker_alias.g");
    let params = params_at(&path, DOC_ALIAS, offset_of(DOC_ALIAS, "as Pt") + 4);
    let mons = monikers(&db, &file_map, &params).expect("the alias binding answers");
    assert_eq!(mons.len(), 1);
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Import));
    assert_eq!(mons[0].identifier, "crate::shapes::Point");
}

/// Two session files recording the same name answer one deduplicated
/// export moniker: identical identifiers collapse, and the answer is
/// deterministic regardless of the database's per-file iteration order.
#[test]
fn same_named_definitions_across_files_collapse() {
    let (db, mut file_map, path) = populated_db(DOC, "/test/moniker_multi.g");
    // Populate file B through the same stores (the driver's `db` is
    // shared across the session's files).
    let content_b = "fn helper() -> i32 { 2 }\n";
    let path_b = PathBuf::from("/test/moniker_multi_b.g");
    let file_id_b = db.file_map.write().get_or_create(&path_b);
    let caller_b = file_map.get_or_create(&path_b);
    assert_eq!(file_id_b, caller_b);
    db.source_maps
        .write()
        .insert(file_id_b, SourceMap::new(file_id_b, content_b.to_string()));
    let parse_b = glyim_frontend::parse_to_syntax(content_b, file_id_b);
    let mut interner_b = glyim_core::Interner::new();
    let (hir_b, _diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_b.root, &mut interner_b);
    db.symbol_index
        .write()
        .build_from_hir(file_id_b, &hir_b, &interner_b);
    db.hirs.write().insert(file_id_b, hir_b);

    let params = params_at(&path, DOC, offset_of(DOC, "fn helper() -> i32") + 3);
    let mons = monikers(&db, &file_map, &params).expect("helper is recorded");
    assert_eq!(mons.len(), 1, "identical identifiers collapse");
    assert_eq!(kind_of(&mons[0]), Some(lsp_types::MonikerKind::Export));
}

/// A document the server does not know (not open, never analysed)
/// answers `null`.
#[test]
fn an_unopened_document_answers_null() {
    let (db, _file_map, _path) = populated_db(DOC, "/test/moniker_closed.g");
    let path = PathBuf::from("/test/never_opened.g");
    let params = params_at(&path, DOC, 0);
    assert!(monikers(&db, &FileMap::new(), &params).is_none());
}

/// The wire contract the engine implements: the lsp-types request type's
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    use lsp_types::request::Request;
    assert_eq!(MonikerRequest::METHOD, "textDocument/moniker");
    let _ = FileId::from_raw(0); // keep the span crate in the test's deps
}
