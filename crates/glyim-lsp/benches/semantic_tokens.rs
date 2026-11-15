//! Semantic-tokens bench (LSP-SEMANTIC-TOKENS).
//!
//! Dependency-free (`harness = false`), mirroring the `type_definition`
//! and `selection_range` benches: measures the semantic-tokens request
//! path end-to-end over a synthetic multi-unit corpus — the
//! classification policy (token walk through the `TokenLexSource` seam)
//! and the full wire request (parse-tree walk → classify → UTF-16 delta
//! encode) — so CI logs can track the per-request cost as the token
//! source grows consumers. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench semantic_tokens
//! ```

use std::hint::black_box;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Instant;

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::semantic_tokens::{
    ParseTreeTokenSource, classify_tokens, semantic_tokens_full, semantic_tokens_range,
};
use lsp_types::{SemanticTokensParams, SemanticTokensRangeParams, TextDocumentIdentifier};
use url::Url;

/// One shape per unit: comment, struct + fields, fn + params, let
/// bindings, a struct literal, and a tail field read — enough tokens of
/// every classified class that the policy sifts a realistic mix.
const UNIT_SOURCE: &str = "// unit\n\
                           struct Shape { tag: i32 }\n\
                           fn render(scale: i32) -> i32 {\n\
                           \x20   let label = \"frame\";\n\
                           \x20   let size = 42;\n\
                           \x20   let frame = Shape { tag: scale };\n\
                           \x20   frame.tag\n\
                           }\n";
const UNITS: usize = 64;

fn main() {
    // Build the corpus: `units` struct/functions, parsed once, the
    // database populated exactly like the analysis driver does (the
    // semantic-tokens path needs the parse tree + source map only).
    let mut source = String::with_capacity(UNITS * UNIT_SOURCE.len());
    for _ in 0..UNITS {
        source.push_str(UNIT_SOURCE);
    }

    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/bench/semantic_tokens.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, source.clone()));
    let parse_result = parse_to_syntax(&source, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "bench fixture must parse cleanly"
    );
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    // Policy bench: classification over the stored parse tree through
    // the seam, no database locks — the unit the policy tests pin.
    // `syntaxes` guard lives in this block only, so the wire bench below
    // starts from a lock-free database.
    {
        let syntaxes = db.syntaxes.read();
        let green = syntaxes.get(&file_id).expect("bench file has a tree");
        let source_impl = ParseTreeTokenSource::new(green);
        let t0 = Instant::now();
        let mut tokens = 0usize;
        for _ in 0..1_000 {
            let classified = classify_tokens(&source_impl);
            tokens += classified.len();
            black_box(&classified);
        }
        println!(
            "policy classify: {:>9.1?}/call (1,000 calls, {tokens} tokens total)",
            t0.elapsed() / 1_000
        );
    }

    // Wire bench: the full request path through a real database, exactly
    // as the router handler drives it (tree walk → classify → UTF-16
    // delta encode).
    let params = SemanticTokensParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("bench fixture path converts to a URI"),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let t1 = Instant::now();
    let mut wire_tokens = 0usize;
    for _ in 0..1_000 {
        let response = semantic_tokens_full(&db, &file_map, &params);
        if let Some(lsp_types::SemanticTokensResult::Tokens(tokens)) = &response {
            wire_tokens += tokens.data.len();
        }
        black_box(&response);
    }
    println!(
        "wire request  : {:>9.1?}/call (1,000 calls, {wire_tokens} wire tokens total)",
        t1.elapsed() / 1_000
    );

    // Range bench: the per-scroll shape — a window over the middle of
    // the corpus, the request editors fire repeatedly while the user
    // scrolls. Measures classify + intersect + re-encode.
    let range_params = SemanticTokensRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("bench fixture path converts to a URI"),
        },
        // A four-line window starting one unit into the corpus.
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 8,
                character: 0,
            },
            end: lsp_types::Position {
                line: 12,
                character: 0,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let t2 = Instant::now();
    let mut range_tokens = 0usize;
    for _ in 0..1_000 {
        let response = semantic_tokens_range(&db, &file_map, &range_params);
        if let Some(lsp_types::SemanticTokensRangeResult::Tokens(tokens)) = &response {
            range_tokens += tokens.data.len();
        }
        black_box(&response);
    }
    println!(
        "range request : {:>9.1?}/call (1,000 calls, {range_tokens} wire tokens total)",
        t2.elapsed() / 1_000
    );
}
