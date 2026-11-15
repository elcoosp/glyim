//! Selection-range bench (LSP-SELECTION).
//!
//! Dependency-free (`harness = false`), mirroring the `document_highlight`
//! bench: measures the selection-range request path end-to-end over a
//! synthetic parsed corpus — the policy (chain normalization through the
//! `AncestorSpanSource` seam) and the full wire request (file lookup →
//! stored-tree lookup → per-position offsets → ancestor walk → wire
//! linked list) — so CI logs can track the per-request cost as the parse
//! trees become a live part of the request surface. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench selection_range
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::SourceMap;
use glyim_lsp::selection_range::{ParseTreeSource, compute_selection_chains, selection_ranges};
use glyim_span::FileId;
use lsp_types::{Position, SelectionRangeParams, TextDocumentIdentifier};
use url::Url;

/// One scope per unit: a function holding a let statement, an assignment,
/// and a call — enough nesting for a token → statement → block → item →
/// file ancestor chain.
const SCOPE_SOURCE: &str = "fn scope() -> i32 {\n    let x = 0;\n    x = x + 1;\n    x\n}\n";
/// Line/column of the `x` binding token inside [`SCOPE_SOURCE`] (line 1,
/// column 8 — after `    let `).
const X_LINE: u32 = 1;
const X_COLUMN: u32 = 8;
const SCOPES: usize = 64;

fn bench_params(path: &PathBuf) -> SelectionRangeParams {
    SelectionRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("bench fixture path converts to a URI"),
        },
        positions: vec![Position {
            line: X_LINE,
            character: X_COLUMN,
        }],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn main() {
    // Build the corpus: `scopes` copies of SCOPE_SOURCE, parsed once, the
    // tree stored in the database exactly like the analysis driver does.
    let mut source = String::with_capacity(SCOPES * SCOPE_SOURCE.len());
    for _ in 0..SCOPES {
        source.push_str(SCOPE_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = glyim_lsp::database::FileMap::new();
    let path = PathBuf::from("/bench/selection.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");

    let parse_result = parse_to_syntax(&source, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "bench corpus must parse cleanly"
    );

    db.syntaxes
        .write()
        .insert(file_id, parse_result.green_node.clone());
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));

    // Byte offset of the same cursor on the wire side (line 1 starts after
    // the first line, `x` sits X_COLUMN UTF-16 units in).
    let first_scope_offset = "fn scope() -> i32 {\n".len() + X_COLUMN as usize;

    // Policy bench: the ancestor walk + chain normalization through the
    // seam, no database locks — the unit the policy tests pin.
    let tree_source = ParseTreeSource::new(&parse_result.green_node, file_id);
    let t0 = Instant::now();
    let mut spans = 0usize;
    for _ in 0..1_000 {
        let chains = compute_selection_chains(&tree_source, &[first_scope_offset]);
        spans += chains.iter().flatten().map(Vec::len).sum::<usize>();
        black_box(&chains);
    }
    let policy = t0.elapsed();
    println!(
        "policy compute : {:>9.1?}/call (1,000 calls, {spans} spans)",
        policy / 1_000
    );

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (read guards over file_map /
    // source_maps / syntaxes included).
    let params = bench_params(&path);
    let t1 = Instant::now();
    let mut wire_len = 0usize;
    for _ in 0..1_000 {
        let ranges = selection_ranges(&db, &file_map, &params).expect("request hits the tree");
        wire_len += ranges.len();
        black_box(&ranges);
    }
    let wire = t1.elapsed();
    println!(
        "wire request   : {:>9.1?}/call (1,000 calls, {wire_len} ranges)",
        wire / 1_000
    );

    black_box((spans, wire_len));
}
