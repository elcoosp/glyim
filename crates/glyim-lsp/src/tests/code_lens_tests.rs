//! Database-backed tests for the code-lens request (LSP-CODE-LENS).
//!
//! Mirrors `document_highlight_tests.rs`: a synthetic `ReferenceGraph` is
//! injected through `insert_test_reference`, a `SourceMap` carries the
//! document text, and the public `code_lenses` / `resolve_code_lens`
//! entry points are exercised end-to-end (URI → open-file lookup →
//! item-level definition walk → wire lenses, then the echoed lens →
//! graph re-join → resolve command). This is the same two-request shape
//! the call-hierarchy engine pins: the client echoes back what the
//! server sent, and the server re-derives the answer from the recorded
//! facts instead of trusting the echo.

use crate::AnalysisDatabase;
use crate::code_lens::{code_lenses, resolve_code_lens};
use crate::database::FileMap;
use crate::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::{
    CodeLens, CodeLensParams, PartialResultParams, Position, TextDocumentIdentifier, Uri,
    WorkDoneProgressParams,
};
use serde_json::json;
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
        ByteIdx::from_raw(lo),
        ByteIdx::from_raw(hi),
        SyntaxContext::ROOT,
    )
}

fn def(file_id: FileId, lo: u32, hi: u32, item_level: bool) -> Reference {
    Reference {
        file_id,
        span: span_of(file_id, lo, hi),
        is_definition: true,
        kind: ReferenceKind::Definition,
        access: AccessKind::Read,
        def_id: None,
        owner_item_id: None,
        is_item_level: item_level,
    }
}

fn call(file_id: FileId, lo: u32, hi: u32) -> Reference {
    Reference {
        file_id,
        span: span_of(file_id, lo, hi),
        is_definition: false,
        kind: ReferenceKind::Call,
        access: AccessKind::Read,
        def_id: None,
        owner_item_id: None,
        is_item_level: true,
    }
}

/// `fn main() { foo(); }` + `fn foo() { 0 }` + `struct Empty { }`: two
/// functions (one called, one uncalled) and a struct, with one item-level
/// def each, plus one scoped (non-item-level) def that must not carry a
/// lens.
const SRC: &str = "fn main() { foo(); }\nfn foo() { 0 }\nstruct Empty { }\n";

/// Byte offsets in `SRC`, derived by needle search (the same
/// expectations-cannot-drift discipline the document-link e2e tests use).
/// Runtime (not `const`) because `str::find` is not const-evaluable.
struct Offsets {
    main_lo: u32,
    main_hi: u32,
    call_lo: u32,
    call_hi: u32,
    foo_lo: u32,
    foo_hi: u32,
    empty_lo: u32,
    empty_hi: u32,
    local_lo: u32,
    local_hi: u32,
}

fn offs() -> Offsets {
    fn at(needle: &str) -> u32 {
        SRC.find(needle)
            .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in SRC")) as u32
    }
    let main_lo = at("main");
    let call_lo = at("foo();");
    let foo_lo = at("fn foo") + 3;
    let empty_lo = at("Empty");
    let local_lo = at("{ 0 }") + 2;
    Offsets {
        main_lo,
        main_hi: main_lo + 4,
        call_lo,
        call_hi: call_lo + 3,
        foo_lo,
        foo_hi: foo_lo + 3,
        empty_lo,
        empty_hi: empty_lo + 5,
        local_lo,
        local_hi: local_lo + 1,
    }
}

/// Map byte offsets in `SRC` to the exact wire range the engine must
/// answer with: the same `SourceMap` line/UTF-16-column mapping the
/// request path uses, so the expectations cannot drift from the mapping
/// under test (a definition on line 1 answers `character = column`, not
/// the global byte offset).
fn lsp_range(lo: u32, hi: u32) -> lsp_types::Range {
    let sm = crate::database::SourceMap::new(FileId::from_raw(0), SRC.to_string());
    let ((start_line, start_col), (end_line, end_col)) = sm
        .span_to_position(lo as usize, hi as usize)
        .unwrap_or_else(|| panic!("fixture extent {lo}..{hi} is not char-boundary safe"));
    lsp_types::Range {
        start: Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    }
}

fn setup() -> (AnalysisDatabase, FileMap, Uri, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/code_lens.gly");
    let uri = uri_for(&path);
    let file_id = file_map.get_or_create(&path);
    db.file_map.write().get_or_create(&path);

    let o = offs();
    let mut graph = ReferenceGraph::new();
    // Item-level definitions: main, foo, Empty — in source order.
    graph.insert_test_reference("main", def(file_id, o.main_lo, o.main_hi, true));
    graph.insert_test_reference("foo", def(file_id, o.foo_lo, o.foo_hi, true));
    graph.insert_test_reference("Empty", def(file_id, o.empty_lo, o.empty_hi, true));
    // One scoped (non-item-level) definition: must not carry a lens.
    graph.insert_test_reference("local", def(file_id, o.local_lo, o.local_hi, false));
    // `main` is never called; `foo` is called once by main.
    graph.insert_test_reference("foo", call(file_id, o.call_lo, o.call_hi));

    db.source_maps.write().insert(
        file_id,
        crate::database::SourceMap::new(file_id, SRC.to_string()),
    );
    *db.reference_graph.write() = graph;

    (db, file_map, uri, file_id)
}

fn lens_params(uri: &Uri) -> CodeLensParams {
    CodeLensParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

/// The `codeLens` response: one lens per item-level definition in source
/// order, exact token extents, no command yet, and the resolve round-trip
/// data payload naming this document.
#[test]
fn code_lenses_answer_per_item_definition_in_source_order() {
    let (db, file_map, uri, _file_id) = setup();
    let o = offs();

    let lenses = code_lenses(&db, &file_map, &lens_params(&uri)).expect("three item-level defs");

    assert_eq!(lenses.len(), 3, "main, foo, Empty — not the scoped local");
    // Source order by definition offset: main, foo, Empty. Wire ranges
    // are exact token extents through the shared SourceMap mapping.
    assert_eq!(lenses[0].range, lsp_range(o.main_lo, o.main_hi));
    assert_eq!(lenses[1].range, lsp_range(o.foo_lo, o.foo_hi));
    assert_eq!(lenses[2].range, lsp_range(o.empty_lo, o.empty_hi));
    // The wire contract: lenses arrive without commands — the editor
    // resolves each one it is about to render.
    assert!(lenses.iter().all(|lens| lens.command.is_none()));
    // The echo payload names the document the resolve round-trips on.
    for lens in &lenses {
        let data = lens.data.as_ref().expect("resolve payload present");
        assert_eq!(data.get("uri").and_then(|v| v.as_str()), Some(uri.as_str()));
    }
}

/// A document the server does not know answers `null` (the engine
/// convention: not-open or never-analysed documents are honest absences).
#[test]
fn unopened_document_answers_none() {
    let (db, file_map, _uri, _file_id) = setup();
    let other = PathBuf::from("/test/not_open.gly");
    let params = lens_params(&uri_for(&other));
    assert!(code_lenses(&db, &file_map, &params).is_none());
}

/// A session with no item-level definitions answers `null`, not an empty
/// array (same convention as document links).
#[test]
fn no_item_definitions_answers_none() {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/no_items.gly");
    let uri = uri_for(&path);
    let file_id = file_map.get_or_create(&path);
    db.file_map.write().get_or_create(&path);
    db.source_maps.write().insert(
        file_id,
        crate::database::SourceMap::new(file_id, "fn {".to_string()),
    );
    assert!(code_lenses(&db, &file_map, &lens_params(&uri)).is_none());
}

/// The resolve round-trip: the echoed lens is re-joined against the graph
/// by definition extent (the call-hierarchy convention) and answers the
/// live use count with correct singular/plural grammar.
#[test]
fn resolve_answers_the_live_use_count() {
    let (db, file_map, uri, _file_id) = setup();
    let o = offs();
    let lenses = code_lenses(&db, &file_map, &lens_params(&uri)).expect("lenses first");
    let foo_lens = lenses
        .iter()
        .find(|lens| lens.range == lsp_range(o.foo_lo, o.foo_hi))
        .expect("foo's lens is in the response")
        .clone();

    let resolved = resolve_code_lens(&db, &file_map, &foo_lens).expect("foo is on the graph");
    let command = resolved.command.expect("resolve attaches the command");
    assert_eq!(command.title, "1 reference");
    assert_eq!(command.command, "glyim.showReferences");
    assert!(
        command.arguments.is_some(),
        "arguments carry the jump target"
    );
    // The echoed fields survive unchanged.
    assert_eq!(resolved.range, foo_lens.range);
    assert_eq!(resolved.data, foo_lens.data);
}

/// `main` has no callers: the resolve answers "0 references" honestly
/// instead of hiding the lens or guessing a count.
#[test]
fn resolve_zero_use_item_answers_zero_references() {
    let (db, file_map, uri, _file_id) = setup();
    let o = offs();
    let lenses = code_lenses(&db, &file_map, &lens_params(&uri)).expect("lenses first");
    let main_lens = lenses
        .iter()
        .find(|lens| lens.range == lsp_range(o.main_lo, o.main_hi))
        .expect("main's lens is in the response")
        .clone();

    let resolved = resolve_code_lens(&db, &file_map, &main_lens).expect("main is on the graph");
    assert_eq!(resolved.command.expect("command").title, "0 references");
}

/// A resolve whose echoed extent matches no item-level definition on the
/// graph (client edited the document between the two requests, or
/// fabricated the lens) answers `null`.
#[test]
fn resolve_unknown_extent_answers_none() {
    let (db, file_map, uri, _file_id) = setup();
    let stranger = CodeLens {
        range: lsp_types::Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 2,
            },
        },
        command: None,
        // The real document URI — only the extent is a stranger.
        data: Some(json!({ "uri": uri.as_str() })),
    };
    assert!(resolve_code_lens(&db, &file_map, &stranger).is_none());
}

/// A resolve without the echo payload (a client that stripped `data`)
/// cannot name the document, so it answers `null` instead of guessing.
#[test]
fn resolve_without_echo_payload_answers_none() {
    let (db, file_map, _uri, _file_id) = setup();
    let o = offs();
    let bare = CodeLens {
        range: lsp_types::Range {
            start: Position {
                line: 0,
                character: o.main_lo,
            },
            end: Position {
                line: 0,
                character: o.main_hi,
            },
        },
        command: None,
        data: None,
    };
    assert!(resolve_code_lens(&db, &file_map, &bare).is_none());
}
