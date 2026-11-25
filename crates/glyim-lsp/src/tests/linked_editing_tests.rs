//! Database-backed tests for the linked-editing-range request
//! (LSP-LINKED-EDIT).
//!
//! Mirrors `document_highlight_tests.rs`: a synthetic `ReferenceGraph` is
//! injected through `insert_test_reference`, a `SourceMap` carries the
//! document text, and the public `linked_editing_ranges` entry point is
//! exercised end-to-end. The *choice* underneath (same-file occurrences,
//! T129 scope-owner filter, DUMMY-span skip) is the document-highlight
//! policy the request reuses — it is pinned once there and once here at
//! the wire boundary, where the kinds are dropped and the ranges become
//! an edit group.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::linked_editing::linked_editing_ranges;
use crate::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::{LinkedEditingRangeParams, Position, TextDocumentIdentifier, Uri};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

fn span_of(file_id: FileId, lo: u32, hi: u32) -> Span {
    Span::range(
        file_id,
        ByteIdx::from_usize(lo as usize),
        ByteIdx::from_usize(hi as usize),
        SyntaxContext::ROOT,
    )
}

fn ref_at(
    file_id: FileId,
    lo: u32,
    hi: u32,
    is_definition: bool,
    access: AccessKind,
    owner_item_id: Option<u32>,
    is_item_level: bool,
) -> Reference {
    Reference {
        file_id,
        span: span_of(file_id, lo, hi),
        is_definition,
        kind: if is_definition {
            ReferenceKind::Definition
        } else {
            ReferenceKind::Call
        },
        access,
        def_id: None,
        owner_item_id,
        is_item_level,
    }
}

/// `fn a() { let x = 1; use_x(x); }` on line 0 and the same shape for
/// `fn b()` on line 1. The two `x` locals are scoped to their own
/// function bodies (owner 1 / owner 2) — linked editing on one must not
/// touch the other, unlike the item-level `foo` fixture.
const SCOPED_SRC: &str = "fn a() { let x = 1; use_x(x); }\nfn b() { let x = 2; use_x(x); }\n";

fn setup_scoped_locals() -> (AnalysisDatabase, FileMap, PathBuf) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/linked_editing.g");
    let file_id = file_map.get_or_create(&path);

    let mut graph = ReferenceGraph::new();
    // Owner 1 (fn a): the binding at 13..14 and its read at 26..27.
    graph.insert_test_reference(
        "x",
        ref_at(file_id, 13, 14, true, AccessKind::Write, Some(1), false),
    );
    graph.insert_test_reference(
        "x",
        ref_at(file_id, 26, 27, false, AccessKind::Read, Some(1), false),
    );
    // Owner 2 (fn b): same name, different scope. Line 1 starts at byte
    // 32, so the same shape repeats +32.
    graph.insert_test_reference(
        "x",
        ref_at(file_id, 45, 46, true, AccessKind::Write, Some(2), false),
    );
    graph.insert_test_reference(
        "x",
        ref_at(file_id, 58, 59, false, AccessKind::Read, Some(2), false),
    );

    let source_map = crate::database::SourceMap::new(file_id, SCOPED_SRC.to_string());
    db.source_maps.write().insert(file_id, source_map);
    *db.reference_graph.write() = graph;

    (db, file_map, path)
}

/// `fn foo() { foo(); }` — the item-level fixture: definition (3..6) and
/// call (11..14), both `is_item_level`.
fn setup_test_db_with_foo() -> (AnalysisDatabase, FileMap, PathBuf) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/linked_editing_foo.g");
    let file_id = file_map.get_or_create(&path);

    let mut graph = ReferenceGraph::new();
    graph.insert_test_reference(
        "foo",
        ref_at(file_id, 3, 6, true, AccessKind::Read, None, true),
    );
    graph.insert_test_reference(
        "foo",
        ref_at(file_id, 11, 14, false, AccessKind::Read, None, true),
    );

    let source_map = crate::database::SourceMap::new(file_id, "fn foo() { foo(); }".to_string());
    db.source_maps.write().insert(file_id, source_map);
    *db.reference_graph.write() = graph;

    (db, file_map, path)
}

fn params_at(path: &PathBuf, line: u32, character: u32) -> LinkedEditingRangeParams {
    LinkedEditingRangeParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_for(path) },
            position: Position { line, character },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    }
}

#[test]
fn item_level_symbol_links_definition_and_call() {
    let (db, file_map, path) = setup_test_db_with_foo();
    let ranges = linked_editing_ranges(&db, &file_map, &params_at(&path, 0, 4))
        .expect("cursor sits on a graphed symbol");

    assert_eq!(ranges.ranges.len(), 2, "definition + call edit together");
    // Exact token extents, sorted by position; the kinds are dropped —
    // linked editing is kind-free by spec.
    let first = &ranges.ranges[0];
    assert_eq!((first.start.line, first.start.character), (0, 3));
    assert_eq!(first.end.character, 6);
    let second = &ranges.ranges[1];
    assert_eq!((second.start.line, second.start.character), (0, 11));
    assert_eq!(second.end.character, 14);
    // The client's default word pattern covers the language's identifier
    // shape, so the server spells none (documented at the entry point).
    assert!(ranges.word_pattern.is_none());
}

#[test]
fn scoped_locals_do_not_link_across_owners() {
    let (db, file_map, path) = setup_scoped_locals();
    // Cursor inside owner 1's read (`use_x(x)` in fn a, line 0 char 26).
    let ranges = linked_editing_ranges(&db, &file_map, &params_at(&path, 0, 26))
        .expect("cursor sits on a scoped occurrence");

    let starts: Vec<(u32, u32)> = ranges
        .ranges
        .iter()
        .map(|r| (r.start.line, r.start.character))
        .collect();
    assert_eq!(
        starts,
        vec![(0, 13), (0, 26)],
        "only owner 1's binding and read edit together"
    );
    // The same request from inside fn b links only owner 2's pair.
    let other = linked_editing_ranges(&db, &file_map, &params_at(&path, 1, 13))
        .expect("cursor sits on owner 2's binding");
    let other_starts: Vec<(u32, u32)> = other
        .ranges
        .iter()
        .map(|r| (r.start.line, r.start.character))
        .collect();
    assert_eq!(other_starts, vec![(1, 13), (1, 26)]);
}

#[test]
fn other_files_occurrences_do_not_leak_into_the_ranges() {
    let (db, file_map, path) = setup_test_db_with_foo();
    // A second file's occurrence of the same item-level name: linked
    // editing is per-document, so it must never appear.
    let other_file = FileId::from_usize(7);
    db.reference_graph.write().insert_test_reference(
        "foo",
        ref_at(other_file, 0, 3, false, AccessKind::Read, None, true),
    );

    let ranges = linked_editing_ranges(&db, &file_map, &params_at(&path, 0, 4))
        .expect("cursor sits on a graphed symbol");
    assert_eq!(
        ranges.ranges.len(),
        2,
        "the other file's occurrence stays out"
    );
}

#[test]
fn dummy_spans_are_skipped() {
    let (db, file_map, path) = setup_test_db_with_foo();
    // Pattern bindings currently have no real span (T053 [LSP-4]); a
    // DUMMY-span occurrence must not become a bogus 0:0 edit range.
    let file_id = file_map.get_by_path(&path).expect("fixture path is mapped");
    db.reference_graph.write().insert_test_reference(
        "foo",
        Reference {
            file_id,
            span: Span::DUMMY,
            is_definition: false,
            kind: ReferenceKind::Call,
            access: AccessKind::Read,
            def_id: None,
            owner_item_id: None,
            is_item_level: true,
        },
    );

    let ranges = linked_editing_ranges(&db, &file_map, &params_at(&path, 0, 4))
        .expect("cursor sits on a graphed symbol");
    assert_eq!(ranges.ranges.len(), 2, "the DUMMY span stays out");
}

#[test]
fn none_off_symbol() {
    let (db, file_map, path) = setup_test_db_with_foo();
    // Whitespace between the name and `{`: no occurrence under the
    // cursor, no edit group.
    assert!(linked_editing_ranges(&db, &file_map, &params_at(&path, 0, 8)).is_none());
}

#[test]
fn none_for_unknown_document() {
    let (db, file_map, _path) = setup_test_db_with_foo();
    let closed = PathBuf::from("/test/never-opened.g");
    assert!(linked_editing_ranges(&db, &file_map, &params_at(&closed, 0, 1)).is_none());
}
