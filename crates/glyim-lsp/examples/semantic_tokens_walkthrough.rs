//! Consumer walkthrough of the semantic-tokens seam
//! (LSP-SEMANTIC-TOKENS): the `textDocument/semanticTokens/full`
//! capability behind an editor's syntax coloring.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example semantic_tokens_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a document is parsed exactly like the analysis driver
//! parses it, the parse tree is stored in `AnalysisDatabase` like the
//! driver stores it, and `semantic_tokens_full` — the function the
//! router registers for `SemanticTokensFullRequest` — classifies the
//! tree's tokens through the `TokenLexSource` seam and delta-encodes
//! them into the spec's flat `u32` wire array. The walkthrough prints
//! the legend the capability advertises, decodes the deltas back to
//! absolute positions so a human can read the response, and shows a
//! closed document answering `null`.

use glyim_frontend::parse_to_syntax;
use glyim_lsp::database::{FileMap, SourceMap};
use glyim_lsp::semantic_tokens::{semantic_tokens_full, semantic_tokens_range, token_legend};
use lsp_types::{
    SemanticToken, SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensResult,
    TextDocumentIdentifier,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

/// The walkthrough document: a comment, a struct with a field, a
/// function with a parameter, let bindings, a struct literal, and a
/// tail field read — every class the policy claims.
const DOC: &str = "// one frame\n\
                   struct Rect { w: i32 }\n\n\
                   fn render(scale: i32) -> i32 {\n\
                   \x20   let label = \"café\";\n\
                   \x20   let size = 42;\n\
                   \x20   let frame = Rect { w: scale };\n\
                   \x20   frame.w\n\
                   }\n";

/// Byte offset of the first `needle` in DOC.
fn main() {
    let db = glyim_lsp::AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/walkthrough/semantic_tokens.g");
    // Both maps allocate ids from 0, so the db-side and caller-side ids
    // for `path` agree (same pattern as the driver's get_or_create).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, DOC.to_string()));

    // The driver's parse step: the semantic-tokens engine walks the
    // stored tree, so this is the only pipeline stage the request needs.
    let parse_result = parse_to_syntax(DOC, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "walkthrough fixture must parse cleanly"
    );
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    let params = SemanticTokensParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("walkthrough path converts to a URI"),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };

    println!(
        "legend: {:?}",
        token_legend()
            .token_types
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
    );
    println!("document: {DOC:?}\n");

    let Some(SemanticTokensResult::Tokens(tokens)) = semantic_tokens_full(&db, &file_map, &params)
    else {
        println!("semanticTokens → null (unexpected for an open document)");
        return;
    };
    println!(
        "semanticTokens → {} tokens, flat wire array of {} u32 slots",
        tokens.data.len(),
        tokens.data.len() * 5
    );

    // Decode the deltas back to absolute (line, utf16 col) starts — the
    // inverse of the spec's encoding rules — so the response is readable.
    let legend = token_legend().token_types;
    let mut line = 0u32;
    let mut col = 0u32;
    for SemanticToken {
        delta_line,
        delta_start,
        length,
        token_type,
        ..
    } in &tokens.data
    {
        line += *delta_line;
        col = if *delta_line == 0 {
            col + *delta_start
        } else {
            *delta_start
        };
        let class = legend.get(*token_type as usize).map_or("?", |t| t.as_str());
        println!("  {line}:{col:<3} len {length:<3} {class}");
    }

    // The range shape (LSP-SEMANTIC-TOKENS): the request editors fire
    // while scrolling — same classification, restricted to the tokens
    // intersecting a screen window, re-encoded from zero.
    let range_params = SemanticTokensRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&path)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("walkthrough path converts to a URI"),
        },
        // Lines 3..5: the `fn render` signature and the first `let`.
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 3,
                character: 0,
            },
            end: lsp_types::Position {
                line: 5,
                character: 0,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    if let Some(lsp_types::SemanticTokensRangeResult::Tokens(ranged)) =
        semantic_tokens_range(&db, &file_map, &range_params)
    {
        let names: Vec<&str> = ranged
            .data
            .iter()
            .map(|tok| {
                legend
                    .get(tok.token_type as usize)
                    .map_or("?", |ty| ty.as_str())
            })
            .collect();
        println!(
            "semanticTokens range (lines 3..5) → {} tokens: {names:?}",
            ranged.data.len()
        );
    }

    // The honest-null case: a document the driver never analysed has no
    // parse tree, and the request answers null like every other engine.
    let closed = PathBuf::from("/walkthrough/never_opened.g");
    let closed_params = SemanticTokensParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&closed)
                .ok()
                .and_then(|u| lsp_types::Uri::from_str(u.as_str()).ok())
                .expect("walkthrough path converts to a URI"),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    println!(
        "\nsemanticTokens on a closed document → {}",
        if semantic_tokens_full(&db, &file_map, &closed_params).is_none() {
            "null (no parse tree)"
        } else {
            "non-null (unexpected)"
        }
    );
}
