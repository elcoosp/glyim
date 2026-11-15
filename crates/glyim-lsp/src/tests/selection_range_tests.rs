//! Database-backed tests for the selection-range request (LSP-SELECTION).
//!
//! Mirrors `document_highlight_tests.rs`: the parse tree is stored in the
//! `AnalysisDatabase` exactly as the analysis driver stores it, a
//! `SourceMap` carries the document text, and the public `selection_ranges`
//! entry point is exercised end-to-end (position resolution → parse-tree
//! ancestor walk → policy normalization → wire linked list).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::selection_range::selection_ranges;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
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

/// `fn main() { let answer = 42; answer }` — one let statement whose
/// identifier extent and statement extent are distinct on line 0.
const DOC: &str = "fn main() { let answer = 42; answer }";

fn setup_test_db_with_parse_tree() -> (AnalysisDatabase, FileMap, PathBuf, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let path = PathBuf::from("/test/selection.gly");
    // Both maps allocate ids from 0, so the db-side and caller-side FileId
    // for `path` agree (same pattern as `test_utils::setup_test_db`).
    let file_id = db.file_map.write().get_or_create(&path);
    let caller_file_id = file_map.get_or_create(&path);
    assert_eq!(file_id, caller_file_id, "file ids must agree");

    // Store the parse tree exactly like the analysis driver does, so the
    // request path walks the real rowan CST (token extents + ancestors).
    let parse_result = parse_to_syntax(DOC, file_id);
    assert!(
        parse_result.diagnostics.is_empty(),
        "fixture must parse cleanly"
    );
    db.syntaxes.write().insert(file_id, parse_result.green_node);

    let source_map = crate::database::SourceMap::new(file_id, DOC.to_string());
    db.source_maps.write().insert(file_id, source_map);

    (db, file_map, path, file_id)
}

fn params_for(path: &PathBuf, positions: &[Position]) -> lsp_types::SelectionRangeParams {
    lsp_types::SelectionRangeParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri_for(path) },
        positions: positions.to_vec(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

/// Byte offsets of the `answer` tokens in [`DOC`] (`fn main() { let ` is
/// 15 bytes, `answer` is 6 wide).
const LET_ANSWER_LO: usize = 16;
const LET_ANSWER_HI: usize = 22;
/// The second `answer` (expression) at the tail of the block.
const USE_ANSWER_LO: usize = 29;
const USE_ANSWER_HI: usize = 35;

#[test]
fn selection_ranges_starts_at_the_token_extent() {
    let (db, file_map, path, _file_id) = setup_test_db_with_parse_tree();
    let params = params_for(
        &path,
        &[Position {
            line: 0,
            character: 17,
        }],
    ); // inside `answer`

    let ranges = selection_ranges(&db, &file_map, &params).expect("cursor sits on a token");

    assert_eq!(ranges.len(), 1);
    let first = &ranges[0];
    assert_eq!(first.range.start.character as usize, LET_ANSWER_LO);
    assert_eq!(first.range.end.character as usize, LET_ANSWER_HI);
}

#[test]
fn selection_ranges_chain_walks_ancestors_to_the_file_root() {
    let (db, file_map, path, _file_id) = setup_test_db_with_parse_tree();
    let params = params_for(
        &path,
        &[Position {
            line: 0,
            character: 17,
        }],
    );

    let ranges = selection_ranges(&db, &file_map, &params).expect("cursor sits on a token");

    // Walk the parent links: they must strictly widen and end at the
    // whole-document range (0..DOC.len() on line 0).
    let doc_end = DOC.len();
    let mut count = 1usize;
    let mut current = ranges[0].parent.as_deref();
    while let Some(range) = current {
        assert!(
            range.range.start <= ranges[0].range.start && range.range.end >= ranges[0].range.end,
            "parents strictly contain the innermost range"
        );
        count += 1;
        current = range.parent.as_deref();
    }
    assert!(count >= 3, "token → statement(s) → file root, got {count}");
    // The outermost parent covers the entire document.
    let mut outer = &ranges[0];
    while let Some(p) = outer.parent.as_deref() {
        outer = p;
    }
    assert_eq!(outer.range.start.line, 0);
    assert_eq!(outer.range.start.character, 0);
    assert_eq!(outer.range.end.character as usize, doc_end);
}

#[test]
fn selection_ranges_are_returned_per_position_in_order() {
    let (db, file_map, path, _file_id) = setup_test_db_with_parse_tree();
    let params = params_for(
        &path,
        &[
            Position {
                line: 0,
                character: 17,
            }, // the let-bound `answer`
            Position {
                line: 0,
                character: 30,
            }, // the tail expression `answer`
        ],
    );

    let ranges = selection_ranges(&db, &file_map, &params).expect("both cursors sit on tokens");

    assert_eq!(ranges.len(), 2, "one result per position, same order");
    assert_eq!(ranges[0].range.start.character as usize, LET_ANSWER_LO);
    assert_eq!(ranges[1].range.start.character as usize, USE_ANSWER_LO);
    assert_eq!(ranges[1].range.end.character as usize, USE_ANSWER_HI);
}

#[test]
fn selection_ranges_none_for_unknown_document() {
    let (db, file_map, _path, _file_id) = setup_test_db_with_parse_tree();
    let closed = PathBuf::from("/test/never-opened.gly");
    let params = params_for(
        &closed,
        &[Position {
            line: 0,
            character: 1,
        }],
    );

    assert!(selection_ranges(&db, &file_map, &params).is_none());
}

#[test]
fn selection_ranges_none_for_empty_position_list() {
    let (db, file_map, path, _file_id) = setup_test_db_with_parse_tree();
    let params = params_for(&path, &[]);

    assert!(selection_ranges(&db, &file_map, &params).is_none());
}

#[test]
fn selection_ranges_at_file_end_still_resolves() {
    // A cursor parked at the very end of the document (offset == len) is
    // legal for rowan's token_at_offset; the innermost selection is the
    // last token (the closing `}`), never a panic.
    let (db, file_map, path, _file_id) = setup_test_db_with_parse_tree();
    let params = params_for(
        &path,
        &[Position {
            line: 0,
            character: DOC.chars().map(char::len_utf16).sum::<usize>() as u32,
        }],
    );

    let ranges = selection_ranges(&db, &file_map, &params).expect("EOF cursor resolves");

    assert_eq!(ranges.len(), 1);
    assert_eq!(
        ranges[0].range.end.character as usize,
        DOC.len(),
        "the last token ends at the document end"
    );
}
