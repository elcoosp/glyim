//! Document-highlight bench (LSP-HIGHLIGHT).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-diag`
//! benches: measures the highlight request path end-to-end over a
//! synthetic occurrence corpus — the policy (scope filtering + kind
//! classification through the `OccurrenceSource` seam) and the full wire
//! request (file lookup → offset → identifier → policy → span→position
//! mapping) — so CI logs can track the per-request cost as the occurrence
//! surface grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench document_highlight
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_lsp::database::SourceMap;
use glyim_lsp::document_highlight::{
    ReferenceGraphSource, compute_highlights, document_highlights,
};
use glyim_lsp::prepare_rename::prepare_rename;
use glyim_lsp::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::{
    DocumentHighlightParams, PartialResultParams, Position, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// One synthetic translation unit per scope: definition + write + read of
/// the local `x` (three occurrences), each scope 33 bytes.
const OCCURRENCES_PER_SCOPE: usize = 3;
const SCOPE_SOURCE: &str = "let x = 0;\nx = 1;\nlet y = x + 1;\n";
/// Byte offsets of the three `x` tokens inside [`SCOPE_SOURCE`].
const X_OFFSETS: [usize; 3] = [4, 11, 26];

/// Builds the corpus graph: `scopes` functions, each holding a local `x`
/// recorded as definition (Read) → write (Write) → read (Read), owner =
/// scope index. This is the shape the T129 scope filter must cut.
fn synthetic_graph(source_len: usize, file_id: FileId, scopes: usize) -> ReferenceGraph {
    let mut graph = ReferenceGraph::new();
    for scope in 0..scopes {
        let base = scope * SCOPE_SOURCE.len();
        for (slot, &off) in X_OFFSETS.iter().enumerate() {
            graph.insert_test_reference(
                "x",
                Reference {
                    file_id,
                    span: Span::range(
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
    debug_assert_eq!(source_len, scopes * SCOPE_SOURCE.len());
    graph
}

fn bench_params(path: &PathBuf) -> DocumentHighlightParams {
    DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: lsp_types::Uri::from_str(
                    &lsp_types::Uri::from_str(Url::from_file_path(path).unwrap().as_ref())
                        .unwrap()
                        .to_string(),
                )
                .unwrap(),
            },
            // On the `x` of `let x = 0;` in the first scope: the definition
            // site, so the response is scope-filtered to owner 0.
            position: Position {
                line: 0,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn main() {
    let scopes = 256; // 768 occurrences: a large-body request shape.
    let source = SCOPE_SOURCE.repeat(scopes);

    // The request path needs a real database first: the synthetic graph
    // must carry the FileId the FileMap actually assigns (not a constant).
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = glyim_lsp::FileMap::new();
    let path = PathBuf::from("/bench/highlight.gly");
    let bench_file = file_map.get_or_create(&path);
    let mut graph = synthetic_graph(source.len(), bench_file, scopes);
    assert_eq!(
        graph.find_references("x").len(),
        scopes * OCCURRENCES_PER_SCOPE,
        "fixture shape drifted"
    );
    db.source_maps
        .write()
        .insert(bench_file, SourceMap::new(bench_file, source.clone()));
    std::mem::swap(&mut *db.reference_graph.write(), &mut graph);

    // Policy bench: scope-filtered compute over the borrowed-graph source
    // (the same shared store the wire path reads). The guard binding keeps
    // the borrow alive for the whole measurement loop.
    let ref_graph_guard = db.reference_graph.read();
    let occurrence_source = ReferenceGraphSource::new(&ref_graph_guard);
    let t0 = Instant::now();
    let mut computed = 0usize;
    for _ in 0..1_000 {
        let hs = compute_highlights(&occurrence_source, bench_file, 4, "x")
            .expect("cursor sits on a definition");
        computed += hs.len();
    }
    let policy = t0.elapsed();
    println!(
        "policy compute : {:>9.1?}/call (1,000 calls, {} highlights each)",
        policy / 1_000,
        computed / 1_000
    );

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (read guards over file_map /
    // source_maps / reference_graph included).
    let params = bench_params(&path);
    let t1 = Instant::now();
    let mut wire_len = 0usize;
    for _ in 0..1_000 {
        let hs = document_highlights(&db, &file_map, &params).expect("request hits a symbol");
        wire_len += hs.len();
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {} highlights each)",
        wire / 1_000,
        wire_len / 1_000
    );

    // Prepare-rename bench (LSP-PREPARE-RENAME): the third consumer of the
    // same occurrence seam — cursor resolution + renameability check +
    // exact-extent wire response.
    let prepare_params = lsp_types::TextDocumentPositionParams {
        text_document: params.text_document_position_params.text_document.clone(),
        position: Position {
            line: 0,
            character: 4,
        },
    };
    let t2 = Instant::now();
    let mut prepared = 0usize;
    for _ in 0..1_000 {
        let target = prepare_rename(&db, &file_map, &prepare_params)
            .expect("cursor sits on a renameable symbol");
        prepared += 1;
        black_box(target);
    }
    let prepare = t2.elapsed();
    println!(
        "prepare rename : {:>9.1?}/call (1,000 calls, {} accepted)",
        prepare / 1_000,
        prepared
    );

    black_box((computed, wire_len));
}
