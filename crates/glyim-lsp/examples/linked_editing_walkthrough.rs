//! Consumer walkthrough of the linked-editing seam (LSP-LINKED-EDIT): the
//! `textDocument/linkedEditingRange` capability behind an editor's
//! multi-cursor "type once, all occurrences follow" editing.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example linked_editing_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a synthetic `ReferenceGraph` — the same store find-references,
//! rename, and document-highlight read — is populated exactly like the
//! analysis driver fills it, and `linked_editing_ranges` — the function
//! the router registers for `LinkedEditingRange` — turns a cursor on one
//! occurrence of a local into the exact extents of every occurrence in
//! its scope, while the same-named local of the *neighbouring scope*
//! stays out of the group (the T129 scope-owner filter the capability
//! composes). Kinds are dropped on the wire: linked editing is
//! kind-free by spec. Every offset is derived from the document by token
//! search, so the walkthrough cannot silently drift from `DOC`.

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::linked_editing::linked_editing_ranges;
use glyim_lsp::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, Span, SyntaxContext};
use lsp_types::{LinkedEditingRangeParams, Position, TextDocumentIdentifier};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// Two neighbouring functions, each holding a local `count` used twice.
/// Same name, different scopes — the shape that separates linked editing
/// from a naive text search.
const DOC: &str = "fn a() {\n  let count = 1;\n  log(count);\n}\n\
                   fn b() {\n  let count = 2;\n  log(count);\n}\n";

fn main() {
    // Every offset comes from token search over `DOC`, so the fixture
    // cannot silently drift from the document.
    let a_binding = DOC.find("let count").expect("walkthrough fixture drift") + "let ".len();
    let a_read = DOC.find("log(count)").expect("walkthrough fixture drift") + "log(".len();
    let b_binding = DOC[a_read + 1..]
        .find("let count")
        .expect("walkthrough fixture drift")
        + a_read
        + 1
        + "let ".len();
    let b_read = DOC[a_read + 1..]
        .find("log(count)")
        .expect("walkthrough fixture drift")
        + a_read
        + 1
        + "log(".len();

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/linked_editing.g");
    let file_id = file_map.get_or_create(&path);
    db.file_map.write().get_or_create(&path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // Populate the graph exactly like the driver does: owner 0 = fn a's
    // body (binding + read), owner 1 = fn b's body (same name, its own
    // scope).
    let mut graph = ReferenceGraph::new();
    for (owner, sites) in [
        (0u32, [(a_binding, true), (a_read, false)]),
        (1u32, [(b_binding, true), (b_read, false)]),
    ] {
        for (off, is_definition) in sites {
            graph.insert_test_reference(
                "count",
                Reference {
                    file_id,
                    span: Span::range(
                        file_id,
                        ByteIdx::from_usize(off),
                        ByteIdx::from_usize(off + "count".len()),
                        SyntaxContext::ROOT,
                    ),
                    is_definition,
                    kind: if is_definition {
                        ReferenceKind::Definition
                    } else {
                        ReferenceKind::Variable
                    },
                    access: if is_definition {
                        AccessKind::Write
                    } else {
                        AccessKind::Read
                    },
                    def_id: None,
                    owner_item_id: Some(owner),
                    is_item_level: false,
                },
            );
        }
    }
    *db.reference_graph.write() = graph;

    // Cursor inside fn a's read of `count` (the `log(count)` site).
    let cursor_offset = a_read + 1;
    let sm = db.source_maps.read();
    let ((line, col), _) = sm
        .get(&file_id)
        .and_then(|sm| sm.span_to_position(cursor_offset, cursor_offset + 1))
        .expect("cursor offset maps");
    drop(sm);

    let params = LinkedEditingRangeParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&path)
                    .ok()
                    .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                    .expect("walkthrough path converts to a URI"),
            },
            position: Position {
                line: line as u32,
                character: col as u32,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };

    let group =
        linked_editing_ranges(&db, &file_map, &params).expect("cursor sits on a scoped local");

    println!("document: {}", path.display());
    println!(
        "edit group for `count` (cursor at {line}:{col}): {} range(s)",
        group.ranges.len()
    );
    for range in &group.ranges {
        println!(
            "  {}:{}..{}:{}",
            range.start.line, range.start.character, range.end.line, range.end.character
        );
    }

    // Honesty pins: the group is exactly fn a's binding + read — fn b's
    // same-named local must never edit together — and the server spells
    // no word pattern (the client default covers identifiers).
    assert_eq!(
        group.ranges.len(),
        2,
        "only fn a's occurrences edit together"
    );
    assert!(group.word_pattern.is_none());
    println!("\nwalkthrough complete: scoped occurrences link, the neighbour scope stays out.");
}
