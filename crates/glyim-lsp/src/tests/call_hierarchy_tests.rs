//! Database-backed tests for the call-hierarchy requests (LSP-CALL-HIERARCHY).
//!
//! Mirrors `type_definition_tests.rs`: the real parser, def-map, HIR, and
//! typeck pipeline populate the database exactly like the analysis driver
//! does, the shared `ReferenceGraph` is built from the real HIR (the same
//! store rename and find-references read), and the public `prepare_call_
//! hierarchy` / `incoming_calls` / `outgoing_calls` entry points — the
//! functions the router registers — are exercised end-to-end. This is also
//! the regression pin for the body-owner fix in the reference graph: call
//! sites used to inherit the *last* HIR item's id as their owner, which
//! resolved every caller to the final function in the file (visible as
//! `incoming_calls` losing `helper` below).

use crate::AnalysisDatabase;
use crate::call_hierarchy::{incoming_calls, outgoing_calls, prepare_call_hierarchy};
use crate::database::{FileMap, SourceMap};
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

/// One struct (proving non-functions stay out), three functions: `leaf`
/// is called twice by `helper` and once by `main`; `helper` is called
/// once by `main`. Lines: 0 struct · 1-3 leaf · 4-6 helper · 7-10 main.
const DOC: &str = "struct Config { name: i32 }\n\
fn leaf(x: i32) -> i32 {\n    x + 1\n}\n\
fn helper(v: i32) -> i32 {\n    leaf(v) + leaf(v + 1)\n}\n\
fn main() {\n    let r = helper(3);\n    leaf(r);\n}\n";

/// Byte offsets the cursors sit on (verified against DOC).
const LEAF_DEF: usize = 31; // `leaf` in `fn leaf(...)`
const HELPER_DEF: usize = 68; // `helper` in `fn helper(...)`
const HELPER_CALL: usize = 144; // `helper(3)` inside main
const LEAF_CALL_IN_MAIN: usize = 159; // `leaf(r)` inside main
const CONFIG_DEF: usize = 7; // `Config` in the struct item
const LOCAL_R: usize = 140; // `r` in `let r = helper(3);`

fn populated_db() -> (AnalysisDatabase, FileMap, PathBuf, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/call_hierarchy.g");
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's exact pipeline (parse → def-map → HIR → typeck →
    // symbol index → reference graph). The call-hierarchy path reads the
    // reference graph and the source maps; the rest is populated to
    // mirror the driver so the request runs against a real session state.
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

#[test]
fn prepare_returns_the_function_under_the_cursor() {
    let (db, file_map, path, _file_id) = populated_db();
    for (offset, expected_name, expected_start_line) in
        [(LEAF_DEF, "leaf", 1u32), (HELPER_DEF, "helper", 4u32)]
    {
        let params = lsp_types::CallHierarchyPrepareParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: uri_for(&path),
                },
                position: position_of(offset),
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        };

        let items = prepare_call_hierarchy(&db, &file_map, &params)
            .unwrap_or_else(|| panic!("{expected_name} is a fn item"));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, expected_name);
        assert_eq!(items[0].kind, lsp_types::SymbolKind::FUNCTION);
        assert_eq!(items[0].range.start.line, expected_start_line);
        assert_eq!(items[0].range.start.character, 0);
        assert_eq!(
            items[0].selection_range, items[0].range,
            "the graph records the whole item extent"
        );
    }
}

#[test]
fn prepare_from_a_call_site_resolves_the_callee() {
    // The cursor sits on `helper(3)` inside main; the hierarchy item is
    // the *callee* (helper's own definition site), so the client's
    // subsequent incoming/outgoing queries run against helper. The same
    // holds for `leaf(r)`, the second call site in main.
    let (db, file_map, path, _file_id) = populated_db();
    for (offset, expected_name, expected_line) in [
        (HELPER_CALL, "helper", 4u32),
        (LEAF_CALL_IN_MAIN, "leaf", 1u32),
    ] {
        let params = lsp_types::CallHierarchyPrepareParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: uri_for(&path),
                },
                position: position_of(offset),
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        };

        let items = prepare_call_hierarchy(&db, &file_map, &params)
            .unwrap_or_else(|| panic!("the cursor at {offset} sits on a call"));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, expected_name);
        assert_eq!(items[0].range.start.line, expected_line);
    }
}

#[test]
fn prepare_none_for_a_struct_or_a_local() {
    // Structs are goto-definition material; locals are not hierarchy
    // items. Both answer `null` (the request result is optional).
    let (db, file_map, path, _file_id) = populated_db();
    for offset in [CONFIG_DEF, LOCAL_R] {
        let params = lsp_types::CallHierarchyPrepareParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: uri_for(&path),
                },
                position: position_of(offset),
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        };
        assert!(
            prepare_call_hierarchy(&db, &file_map, &params).is_none(),
            "offset {offset} is not a call-hierarchy target"
        );
    }
}

#[test]
fn incoming_calls_list_helper_and_main_for_leaf() {
    // The body-owner regression pin: `leaf` is called from helper's body
    // (twice) and main's body (once). Before the owner fix every body
    // ref carried the last item's id, so both call sites resolved to
    // main and helper vanished from the response.
    let (db, file_map, path, _file_id) = populated_db();
    let target = prepared_leaf_item(&db, &file_map, &path);

    let calls = incoming_calls(
        &db,
        &file_map,
        &lsp_types::CallHierarchyIncomingCallsParams {
            item: target,
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        },
    )
    .expect("leaf resolves");

    assert_eq!(calls.len(), 2, "helper and main");
    assert_eq!(calls[0].from.name, "helper", "sorted by caller name");
    assert_eq!(calls[0].from_ranges.len(), 2, "helper calls leaf twice");
    assert_eq!(calls[0].from_ranges[0].start.line, 5);
    assert_eq!(calls[1].from.name, "main");
    assert_eq!(calls[1].from_ranges.len(), 1);
    assert_eq!(calls[1].from_ranges[0].start.line, 9);
}

#[test]
fn outgoing_calls_from_main_list_helper_and_leaf() {
    // Prepared the way a client does: cursor on main's own definition.
    let (db, file_map, path, _file_id) = populated_db();
    let main_def = DOC.find("fn main").expect("main is in DOC") + 3;
    let params = lsp_types::CallHierarchyPrepareParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&path),
            },
            position: position_of(main_def),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    let items = prepare_call_hierarchy(&db, &file_map, &params).expect("main is a fn item");

    let calls = outgoing_calls(
        &db,
        &file_map,
        &lsp_types::CallHierarchyOutgoingCallsParams {
            item: items.into_iter().next().expect("one item"),
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        },
    )
    .expect("main resolves");

    assert_eq!(calls.len(), 2, "helper and leaf, grouped by callee");
    assert_eq!(calls[0].to.name, "helper", "sorted by callee name");
    assert_eq!(calls[1].to.name, "leaf");
}

#[test]
fn outgoing_calls_from_leaf_are_empty() {
    // `leaf` calls nothing: a resolved item with no call edges answers
    // an empty list (not `null`) — the empty list is the honest "no
    // callees", `null` is reserved for an unresolvable item.
    let (db, file_map, path, _file_id) = populated_db();
    let target = prepared_leaf_item(&db, &file_map, &path);

    let calls = outgoing_calls(
        &db,
        &file_map,
        &lsp_types::CallHierarchyOutgoingCallsParams {
            item: target,
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        },
    )
    .expect("leaf resolves");

    assert!(calls.is_empty());
}

#[test]
fn incoming_none_for_an_item_the_server_never_prepared() {
    // A client echoing a fabricated item (wrong name, or a name the
    // graph records no item-level definition for) must get `null`, not
    // a hierarchy computed against a guess.
    let (db, file_map, path, _file_id) = populated_db();
    let ghost = lsp_types::CallHierarchyItem {
        name: "ghost".to_string(),
        kind: lsp_types::SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: uri_for(&path),
        range: lsp_types::Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 5,
            },
        },
        selection_range: lsp_types::Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 5,
            },
        },
        data: None,
    };

    assert!(
        incoming_calls(
            &db,
            &file_map,
            &lsp_types::CallHierarchyIncomingCallsParams {
                item: ghost,
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                partial_result_params: lsp_types::PartialResultParams::default(),
            },
        )
        .is_none()
    );
}

/// Prepare `leaf` through the public entry point (the same way a client
/// would) and return the item for the follow-up incoming/outgoing calls.
fn prepared_leaf_item(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    path: &PathBuf,
) -> lsp_types::CallHierarchyItem {
    let params = lsp_types::CallHierarchyPrepareParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
            position: position_of(LEAF_DEF),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    prepare_call_hierarchy(db, file_map, &params)
        .expect("leaf is a fn item")
        .into_iter()
        .next()
        .expect("one hierarchy item")
}
