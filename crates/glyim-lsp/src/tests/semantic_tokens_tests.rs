//! Database-backed tests for the semantic-tokens request
//! (LSP-SEMANTIC-TOKENS).
//!
//! Mirrors `selection_range_tests.rs`: the parse tree is stored in the
//! `AnalysisDatabase` exactly as the analysis driver stores it, a
//! `SourceMap` carries the document text, and the public
//! `semantic_tokens_full` entry point is exercised end-to-end (parse-tree
//! token walk → policy classification → wire delta encoding). The
//! expected absolute positions are derived from `DOC` by token search +
//! an independent UTF-16 column helper, so the assertions cannot silently
//! drift from the fixture; the wire shape (delta line / delta start /
//! UTF-16 length) is decoded back and pinned, including the
//! multi-byte-literal regression (lengths count UTF-16 code units, not
//! bytes — INF-11).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::semantic_tokens::{semantic_tokens_full, token_legend};
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
use lsp_types::{SemanticToken, SemanticTokensResult, Uri};
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

/// A document exercising every classified shape: a comment, a struct
/// declaration, a function with a parameter and return type, let
/// bindings (one string with a multi-byte character, one number), a
/// struct literal, and a tail field read.
const DOC: &str = "// one frame\n\
                   struct Rect { w: i32 }\n\n\
                   fn render(scale: i32) -> i32 {\n\
                   \x20   let label = \"café\";\n\
                   \x20   let size = 42;\n\
                   \x20   let frame = Rect { w: scale };\n\
                   \x20   frame.w\n\
                   }\n";

/// Byte offset of the first `needle` in DOC (panics if the fixture
/// drifts — the walkthrough's discipline).
fn find(needle: &str) -> usize {
    DOC.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not found"))
}

/// Byte offset → (line, utf16 column); derived from DOC, the inverse of
/// `SourceMap::line_col_to_offset`.
fn line_col_of(offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut last_nl = 0usize;
    for (i, ch) in DOC.char_indices() {
        if i == offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    let character = DOC[last_nl..offset]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>();
    (line, character as u32)
}

/// UTF-16 length of the `needle` in DOC.
fn utf16_len_of(needle: &str) -> u32 {
    needle.chars().map(char::len_utf16).sum::<usize>() as u32
}

fn setup_test_db() -> (AnalysisDatabase, FileMap, PathBuf, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/semantic.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side
    // FileId for `path` agree (same pattern as `test_utils`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");

    // Store the parse tree exactly like the analysis driver does, so the
    // request path walks the real rowan CST.
    let parse_result = parse_to_syntax(DOC, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "fixture must parse cleanly: {:?}",
        parse_result.diagnostics
    );
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    let source_map = crate::database::SourceMap::new(file_id, DOC.to_string());
    db.source_maps.write().insert(file_id, source_map);

    (db, file_map, path, file_id)
}

/// Decode the wire deltas back to absolute `(line, utf16 col)` starts
/// (the inverse of the spec's encoding rules) for positional asserts.
fn decode(data: &[SemanticToken]) -> Vec<(u32, u32, u32, u32)> {
    let mut line = 0u32;
    let mut col = 0u32;
    data.iter()
        .map(|t| {
            line += t.delta_line;
            col = if t.delta_line == 0 {
                col + t.delta_start
            } else {
                t.delta_start
            };
            (line, col, t.length, t.token_type)
        })
        .collect()
}

fn tokens_of(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    path: &PathBuf,
) -> Vec<(u32, u32, u32, u32)> {
    let params = lsp_types::SemanticTokensParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let response = semantic_tokens_full(db, file_map, &params).expect("open file answers");
    match response {
        SemanticTokensResult::Tokens(tokens) => decode(&tokens.data),
        SemanticTokensResult::Partial(_) => panic!("unexpected partial result"),
    }
}

/// Legend index of a standard token type by name — keeps the positional
/// asserts honest against legend reordering.
fn legend_index(name: &str) -> u32 {
    token_legend()
        .token_types
        .iter()
        .position(|t| t.as_str() == name)
        .unwrap_or_else(|| panic!("legend missing `{name}`")) as u32
}

#[test]
fn tokens_are_strictly_ordered_wire_shape() {
    let (db, file_map, path, _id) = setup_test_db();
    let tokens = tokens_of(&db, &file_map, &path);

    assert!(!tokens.is_empty(), "the document classifies tokens");
    for pair in tokens.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(
            (b.0 > a.0) || (b.0 == a.0 && b.1 > a.1),
            "decoded tokens must be strictly increasing: {a:?} then {b:?}"
        );
    }
}

#[test]
fn declarations_and_bindings_carry_their_classes() {
    let (db, file_map, path, _id) = setup_test_db();
    let tokens = tokens_of(&db, &file_map, &path);

    // (byte offset of the token, classified-text needle, legend name).
    // Offsets come from `find` so they cannot drift from the fixture;
    // the needle names the exact token text whose UTF-16 length the
    // wire must carry.
    let expect: [(usize, &str, &str); 10] = [
        (find("struct"), "struct", "keyword"),
        (find("Rect {"), "Rect", "type"),
        (find("render"), "render", "function"),
        (find("scale: i32"), "scale", "parameter"),
        (find("i32"), "i32", "type"), // first hit: the param's type path
        (find("label"), "label", "variable"),
        (find("size"), "size", "variable"),
        // `frame` first appears inside the line-0 comment; anchor on the
        // `let` keyword and step to the binding name.
        (find("let frame") + 4, "frame", "variable"),
        (find("frame.w"), "frame", "variable"), // the tail path reference
        (find("frame.w") + 6, "w", "property"), // the tail field read
    ];
    for (offset, token_text, class) in expect {
        let (line, col) = line_col_of(offset);
        let name = legend_index(class);
        assert!(
            tokens.contains(&(line, col, utf16_len_of(token_text), name)),
            "expected a {class} token at {line}:{col} for `{token_text}`; got {tokens:?}"
        );
    }
}

#[test]
fn multibyte_literal_length_is_utf16_not_bytes() {
    let (db, file_map, path, _id) = setup_test_db();
    let tokens = tokens_of(&db, &file_map, &path);
    let string = legend_index("string");

    let offset = find("\"café\"");
    let (line, col) = line_col_of(offset);
    // `"café"` is 6 UTF-16 units but 7 bytes; INF-11 pins the former.
    assert!(
        tokens.contains(&(line, col, 6, string)),
        "string literal length must be UTF-16 (6), got {tokens:?}"
    );
}

#[test]
fn comments_and_literals_classify() {
    let (db, file_map, path, _id) = setup_test_db();
    let tokens = tokens_of(&db, &file_map, &path);
    let comment = legend_index("comment");
    let number = legend_index("number");
    let keyword = legend_index("keyword");

    let offset = find("// one frame");
    let (line, col) = line_col_of(offset);
    // The lexer folds the trailing newline into line comments, so the
    // token spans 13 UTF-16 units ("// one frame" plus `\n`).
    assert!(
        tokens.contains(&(line, col, 13, comment)),
        "comment token at {line}:{col}, got {tokens:?}"
    );

    let offset = find("42");
    let (line, col) = line_col_of(offset);
    assert!(
        tokens.contains(&(line, col, 2, number)),
        "number token at {line}:{col}, got {tokens:?}"
    );

    let offset = find("let");
    let (line, col) = line_col_of(offset);
    assert!(
        tokens.contains(&(line, col, 3, keyword)),
        "keyword token at {line}:{col}, got {tokens:?}"
    );
}

#[test]
fn unopened_document_answers_null() {
    let (db, file_map, _path, _id) = setup_test_db();
    let missing = PathBuf::from("/test/never_opened.gly");
    let params = lsp_types::SemanticTokensParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: uri_for(&missing),
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    assert!(
        semantic_tokens_full(&db, &file_map, &params).is_none(),
        "a document that was never opened has no parse tree"
    );
}

#[test]
fn wire_round_trip_preserves_the_flat_u32_shape() {
    // lsp-types serializes `SemanticTokens.data` as a flat u32 array; the
    // round-trip contract pins the wire shape end-to-end (the same
    // discipline the bench harness applies to its reports).
    let tokens = SemanticTokensResult::Tokens(lsp_types::SemanticTokens {
        result_id: None,
        data: vec![
            SemanticToken {
                delta_line: 0,
                delta_start: 2,
                length: 3,
                token_type: legend_index("keyword"),
                token_modifiers_bitset: 0,
            },
            SemanticToken {
                delta_line: 1,
                delta_start: 4,
                length: 5,
                token_type: legend_index("variable"),
                token_modifiers_bitset: 0,
            },
        ],
    });
    let json = serde_json::to_string(&tokens).expect("serializes");
    assert!(
        json.contains("\"data\":[0,2,3,"),
        "flat u32 array on the wire, got {json}"
    );
    let back: SemanticTokensResult = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, tokens, "round trip is lossless");
}

// --- range request (LSP-SEMANTIC-TOKENS, M2) ---

use crate::semantic_tokens::semantic_tokens_range;

/// Tokens of one range request, decoded to absolute
/// `(line, utf16 col, length, legend)` quadruples.
fn range_tokens_of(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    path: &PathBuf,
    range: lsp_types::Range,
) -> Vec<(u32, u32, u32, u32)> {
    let params = lsp_types::SemanticTokensRangeParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
        range,
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let response =
        semantic_tokens_range(db, file_map, &params).expect("an intersecting range answers");
    match response {
        lsp_types::SemanticTokensRangeResult::Tokens(tokens) => decode(&tokens.data),
        lsp_types::SemanticTokensRangeResult::Partial(_) => panic!("unexpected partial result"),
    }
}

#[test]
fn range_request_returns_the_intersecting_subset() {
    let (db, file_map, path, _id) = setup_test_db();
    // Lines 3..5: the `fn render` signature and the first `let` — the
    // screen range an editor requests while those lines are visible.
    let range = lsp_types::Range {
        start: lsp_types::Position {
            line: 3,
            character: 0,
        },
        end: lsp_types::Position {
            line: 5,
            character: 0,
        },
    };
    let ranged = range_tokens_of(&db, &file_map, &path, range);
    let full = tokens_of(&db, &file_map, &path);

    assert!(!ranged.is_empty(), "lines 3-4 carry classified tokens");
    assert!(
        ranged.len() < full.len(),
        "a mid-document range is a strict subset of the full response"
    );
    for token in &ranged {
        assert!(
            (3..=4).contains(&token.0),
            "range response stays inside the requested lines, got {ranged:?}"
        );
        assert!(
            full.contains(token),
            "every range token is a full-response token, got {token:?}"
        );
    }
}

#[test]
fn range_deltas_restart_from_zero() {
    let (db, file_map, path, _id) = setup_test_db();
    // The encoding is relative to the response's own first token, so a
    // mid-document range re-encodes: its first delta_line is the
    // absolute line (3), not an offset from the file top.
    let range = lsp_types::Range {
        start: lsp_types::Position {
            line: 4,
            character: 0,
        },
        end: lsp_types::Position {
            line: 5,
            character: 0,
        },
    };
    let ranged = range_tokens_of(&db, &file_map, &path, range);
    assert!(!ranged.is_empty(), "line 4 carries classified tokens");
    assert_eq!(
        ranged[0].0, 4,
        "the first delta_line encodes the absolute line of the subset's start"
    );
}

#[test]
fn empty_range_answers_null() {
    let (db, file_map, path, _id) = setup_test_db();
    let degenerate = lsp_types::Range {
        start: lsp_types::Position {
            line: 2,
            character: 0,
        },
        end: lsp_types::Position {
            line: 2,
            character: 0,
        },
    };
    let params = lsp_types::SemanticTokensRangeParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: uri_for(&path),
        },
        range: degenerate,
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    assert!(
        semantic_tokens_range(&db, &file_map, &params).is_none(),
        "an empty intersection has nothing to answer"
    );
}

#[test]
fn unopened_document_range_answers_null() {
    let (db, file_map, _path, _id) = setup_test_db();
    let missing = PathBuf::from("/test/never_opened_range.gly");
    let params = lsp_types::SemanticTokensRangeParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: uri_for(&missing),
        },
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 0,
                character: 0,
            },
            end: lsp_types::Position {
                line: 3,
                character: 0,
            },
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    assert!(
        semantic_tokens_range(&db, &file_map, &params).is_none(),
        "a document that was never opened has no parse tree"
    );
}
