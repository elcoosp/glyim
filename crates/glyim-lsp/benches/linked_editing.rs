//! Linked-editing-range bench (LSP-LINKED-EDIT).
//!
//! Dependency-free (`harness = false`), mirroring the `document_highlight`
//! bench: measures the linked-editing request path end-to-end over a
//! synthetic occurrence corpus — the composed policy (the document-highlight
//! scope filter through the `OccurrenceSource` seam) and the full wire
//! request (file lookup → offset → identifier → policy → kind-free range
//! group) — so CI logs can track the per-request cost as the edit-group
//! surface grows, directly comparable against the highlight numbers. Run
//! with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench linked_editing
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::document_highlight::{ReferenceGraphSource, compute_highlights};
use glyim_lsp::linked_editing::linked_editing_ranges;
use glyim_lsp::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::{LinkedEditingRangeParams, Position, TextDocumentIdentifier};
use url::Url;

const SCOPE_SOURCE: &str = "let x = 0;\nx = 1;\nlet y = x + 1;\n";
/// Byte offsets of the three `x` tokens inside [`SCOPE_SOURCE`].
const X_OFFSETS: [usize; 3] = [4, 11, 26];
/// Scopes the corpus holds; the T129 scope filter must cut all but one.
const SCOPES: usize = 64;

/// Builds the corpus graph: `scopes` functions, each holding a local `x`
/// recorded as definition (Read) → write (Write) → read (Read), owner =
/// scope index. This is the shape the T129 scope filter must cut.
fn synthetic_graph(file_id: FileId, scopes: usize) -> ReferenceGraph {
    let mut graph = ReferenceGraph::new();
    for scope in 0..scopes {
        let base = scope * SCOPE_SOURCE.len();
        for (slot, &off) in X_OFFSETS.iter().enumerate() {
            graph.insert_test_reference(
                "x",
                Reference {
                    file_id,
                    span: Span::new(
                        file_id,
                        ByteIdx::from_usize(base + off),
                        ByteIdx::from_usize(base + off + 1),
                        SyntaxContext::ROOT,
                    ),
                    is_definition: slot == 0,
                    kind: if slot == 0 {
                        ReferenceKind::Definition
                    } else {
                        ReferenceKind::Variable
                    },
                    access: match slot {
                        0 | 2 => AccessKind::Read,
                        _ => AccessKind::Write,
                    },
                    def_id: None,
                    owner_item_id: Some(scope as u32),
                    is_item_level: false,
                },
            );
        }
    }
    graph
}

fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/linked_editing.g");
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");

    let mut source = String::with_capacity(SCOPES * SCOPE_SOURCE.len());
    for _ in 0..SCOPES {
        source.push_str(SCOPE_SOURCE);
    }
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));
    let graph = synthetic_graph(file_id, SCOPES);
    *db.reference_graph.write() = graph;

    // Cursor on the middle occurrence (`x = 1;`) of the LAST scope, so the
    // request has the full corpus of same-name locals to cut through.
    let last_scope = SCOPES - 1;
    let params = LinkedEditingRangeParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&path)
                    .ok()
                    .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                    .expect("bench fixture path converts to a URI"),
            },
            position: Position {
                // Line 1 of the last scope spells `x = 1;` — its `x` is
                // the line's first character (X_OFFSETS[1] is the
                // scope-relative *byte* offset the policy bench uses; the
                // wire request speaks line/column).
                line: (last_scope * 3 + 1) as u32,
                character: 0,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };

    // Policy bench: the composed occurrence policy through the seam, no
    // database locks — the unit the document-highlight policy tests pin.
    // The guard lives in this block only, so the wire bench below starts
    // from a lock-free database.
    {
        let ref_graph = db.reference_graph.read();
        let occurrence_source = ReferenceGraphSource::new(&ref_graph);
        let t0 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..1_000 {
            let highlights = compute_highlights(
                &occurrence_source,
                file_id,
                last_scope * SCOPE_SOURCE.len() + X_OFFSETS[1],
                "x",
            );
            hits += highlights.as_ref().map_or(0, Vec::len);
            black_box(&highlights);
        }
        let policy = t0.elapsed();
        println!(
            "policy compute : {:>9.1?}/call (1,000 calls, {hits} occurrences in scope)",
            policy / 1_000
        );
    };

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (file lookup → offset → identifier
    // → policy → kind-free range group).
    let t1 = Instant::now();
    let mut wire_ranges = 0usize;
    for _ in 0..1_000 {
        let response =
            linked_editing_ranges(&db, &file_map, &params).expect("request hits a scoped local");
        wire_ranges += response.ranges.len();
        black_box(&response);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_ranges} ranges mapped)",
        wire / 1_000
    );

    black_box(wire_ranges);
}
