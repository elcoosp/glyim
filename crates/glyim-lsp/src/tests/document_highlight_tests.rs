//! Database-backed tests for the document-highlight request (LSP-HIGHLIGHT).
//!
//! Mirrors `references_tests.rs`: a synthetic `ReferenceGraph` is injected
//! through `insert_test_reference`, a `SourceMap` carries the document
//! text, and the public `document_highlights` entry point is exercised
//! end-to-end (cursor resolution → policy → wire ranges/kinds).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::document_highlight::document_highlights;
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

/// `fn foo() { foo(); }` with the graph recording the definition (0..3) and
/// the call (11..14) of `foo`, both item-level.
fn setup_test_db_with_foo() -> (AnalysisDatabase, FileMap, PathBuf) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/highlight.gly");
    let file_id = file_map.get_or_create(&path);

    let mut graph = ReferenceGraph::new();
    graph.insert_test_reference(
        "foo",
        Reference {
            file_id,
            span: span_of(file_id, 3, 6), // the real "foo" token in the source
            is_definition: true,
            kind: ReferenceKind::Definition,
            access: AccessKind::Read,
            def_id: None,
            owner_item_id: None,
            is_item_level: true,
        },
    );
    graph.insert_test_reference(
        "foo",
        Reference {
            file_id,
            span: span_of(file_id, 11, 14),
            is_definition: false,
            kind: ReferenceKind::Call,
            access: AccessKind::Read,
            def_id: None,
            owner_item_id: None,
            is_item_level: true,
        },
    );

    let source_map = crate::database::SourceMap::new(file_id, "fn foo() { foo(); }".to_string());
    db.source_maps.write().insert(file_id, source_map);
    *db.reference_graph.write() = graph;

    (db, file_map, path)
}

#[test]
fn document_highlights_returns_wire_ranges_and_kinds() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&path),
            },
            position: Position {
                line: 0,
                character: 4,
            }, // inside the definition "foo"
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    let highlights =
        document_highlights(&db, &file_map, &params).expect("cursor sits on a graphed symbol");

    assert_eq!(
        highlights.len(),
        2,
        "definition + call are both highlighted"
    );

    // Both spans are byte 3..6 and 11..14 on line 0; the wire ranges must
    // carry the exact token extents (rename-style two-tuple mapping, not
    // the find-references `col+1` approximation).
    let first = &highlights[0];
    assert_eq!(first.range.start.line, 0);
    assert_eq!(first.range.start.character, 3);
    assert_eq!(first.range.end.character, 6, "definition span is exact");
    assert_eq!(first.kind, Some(lsp_types::DocumentHighlightKind::TEXT));

    let second = &highlights[1];
    assert_eq!(second.range.start.character, 11);
    assert_eq!(second.range.end.character, 14, "call span is exact");
    assert_eq!(second.kind, Some(lsp_types::DocumentHighlightKind::READ));
}

#[test]
fn document_highlights_from_the_call_site() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&path),
            },
            position: Position {
                line: 0,
                character: 12,
            }, // inside the call
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    let highlights =
        document_highlights(&db, &file_map, &params).expect("cursor sits on a graphed symbol");

    assert_eq!(
        highlights.len(),
        2,
        "same set regardless of which site is under the cursor"
    );
}

#[test]
fn document_highlights_none_off_symbol() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&path),
            },
            position: Position {
                line: 0,
                character: 8,
            }, // whitespace between name and `{`
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    assert!(document_highlights(&db, &file_map, &params).is_none());
}

#[test]
fn document_highlights_none_for_unknown_document() {
    let (db, file_map, _path) = setup_test_db_with_foo();
    // A URI never opened: no FileId, no highlights.
    let closed = PathBuf::from("/test/never-opened.gly");
    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&closed),
            },
            position: Position {
                line: 0,
                character: 1,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    assert!(document_highlights(&db, &file_map, &params).is_none());
}

#[test]
fn write_access_classifies_as_write_kind() {
    // `mut x = 0; x = 1;` shape: definition (Write) + assignment (Write).
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/writes.gly");
    let file_id = file_map.get_or_create(&path);

    let mut graph = ReferenceGraph::new();
    let mut def = Reference {
        file_id,
        span: span_of(file_id, 4, 5), // the "x" token in `let x`
        is_definition: true,
        kind: ReferenceKind::Definition,
        access: AccessKind::Write,
        def_id: None,
        owner_item_id: Some(1),
        is_item_level: false,
    };
    // A definition's Text classification dominates a Write access.
    graph.insert_test_reference("x", def.clone());
    def.is_definition = false;
    def.kind = ReferenceKind::Variable;
    def.span = span_of(file_id, 11, 12); // the "x" token in `x = 1`
    graph.insert_test_reference("x", def);

    let source_map = crate::database::SourceMap::new(file_id, "let x = 0; x = 1;".to_string());
    db.source_maps.write().insert(file_id, source_map);
    *db.reference_graph.write() = graph;

    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: uri_for(&path),
            },
            position: Position {
                line: 0,
                character: 11,
            }, // on the write
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    let highlights =
        document_highlights(&db, &file_map, &params).expect("cursor sits on a graphed symbol");

    assert_eq!(highlights.len(), 2);
    assert_eq!(
        highlights[0].kind,
        Some(lsp_types::DocumentHighlightKind::TEXT)
    );
    assert_eq!(
        highlights[1].kind,
        Some(lsp_types::DocumentHighlightKind::WRITE)
    );
}
