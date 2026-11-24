//! Database-backed tests for the prepare-rename request (LSP-PREPARE-RENAME).
//!
//! Mirrors `references_tests.rs` / `document_highlight_tests.rs`: a
//! synthetic `ReferenceGraph` is injected through `insert_test_reference`
//! and the public `prepare_rename` entry point is exercised end-to-end
//! (cursor resolution → occurrence check → exact-extent wire response).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::prepare_rename::prepare_rename;
use crate::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::Position;
use lsp_types::Uri;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(
        &Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
            .unwrap()
            .to_string(),
    )
    .unwrap()
}

fn span_of(file_id: FileId, lo: u32, hi: u32) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_usize(lo as usize),
        ByteIdx::from_usize(hi as usize),
        SyntaxContext::ROOT,
    )
}

fn reference(file_id: FileId, lo: u32, hi: u32, is_definition: bool) -> Reference {
    Reference {
        file_id,
        span: span_of(file_id, lo, hi),
        is_definition,
        kind: if is_definition {
            ReferenceKind::Definition
        } else {
            ReferenceKind::Call
        },
        access: AccessKind::Read,
        def_id: None,
        owner_item_id: None,
        is_item_level: true,
    }
}

/// `fn foo() { foo(); }` with the graph recording `foo` (definition 3..6,
/// call 11..14, both item-level).
fn setup_test_db_with_foo() -> (AnalysisDatabase, FileMap, PathBuf) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/prepare.gly");
    let file_id = file_map.get_or_create(&path);

    let mut graph = ReferenceGraph::new();
    graph.insert_test_reference("foo", reference(file_id, 3, 6, true));
    graph.insert_test_reference("foo", reference(file_id, 11, 14, false));

    let source_map = crate::database::SourceMap::new(file_id, "fn foo() { foo(); }".to_string());
    db.source_maps.write().insert(file_id, source_map);
    *db.reference_graph.write() = graph;

    (db, file_map, path)
}

fn params_for(path: &PathBuf, position: Position) -> lsp_types::TextDocumentPositionParams {
    lsp_types::TextDocumentPositionParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
        position,
    }
}

#[test]
fn prepare_rename_returns_range_with_placeholder() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let params = params_for(
        &path,
        Position {
            line: 0,
            character: 12,
        },
    ); // on the call

    let response = prepare_rename(&db, &file_map, &params).expect("cursor on a graphed symbol");
    let lsp_types::PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response
    else {
        panic!("expected RangeWithPlaceholder, got {response:?}");
    };
    assert_eq!(range.start.character, 11, "token starts at its own extent");
    assert_eq!(range.end.character, 14, "token ends at its own extent");
    assert_eq!(placeholder, "foo", "the placeholder pre-fills the new name");
}

#[test]
fn prepare_rename_from_the_definition_site() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let params = params_for(
        &path,
        Position {
            line: 0,
            character: 4,
        },
    ); // on the definition

    let response = prepare_rename(&db, &file_map, &params).expect("cursor on a graphed symbol");
    let lsp_types::PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response
    else {
        panic!("expected RangeWithPlaceholder, got {response:?}");
    };
    assert_eq!(range.start.character, 3);
    assert_eq!(range.end.character, 6);
    assert_eq!(placeholder, "foo");
}

#[test]
fn prepare_rename_none_for_ungraphed_identifier() {
    let (db, file_map, path) = setup_test_db_with_foo();
    // `fn` (offset 0..2) is identifier-shaped but has no occurrences.
    let params = params_for(
        &path,
        Position {
            line: 0,
            character: 1,
        },
    );

    assert!(
        prepare_rename(&db, &file_map, &params).is_none(),
        "an ungraphed identifier must not be marked renameable"
    );
}

#[test]
fn prepare_rename_none_off_identifier() {
    let (db, file_map, path) = setup_test_db_with_foo();
    // Whitespace between `{` and `foo` — no identifier under the cursor.
    let params = params_for(
        &path,
        Position {
            line: 0,
            character: 8,
        },
    );

    assert!(prepare_rename(&db, &file_map, &params).is_none());
}

#[test]
fn prepare_rename_none_for_unknown_document() {
    let (db, file_map, _path) = setup_test_db_with_foo();
    let closed = PathBuf::from("/test/never-opened.gly");
    let params = params_for(
        &closed,
        Position {
            line: 0,
            character: 4,
        },
    );

    assert!(prepare_rename(&db, &file_map, &params).is_none());
}
