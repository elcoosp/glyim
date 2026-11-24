//! Database-backed tests for the on-type-formatting request
//! (LSP-ON-TYPE-FORMAT).
//!
//! Mirrors `document_link_tests.rs`: the parse tree is stored in the
//! `AnalysisDatabase` exactly as the analysis driver stores it, a
//! `SourceMap` carries the document text, and the public
//! `on_type_formatting` entry point is exercised end-to-end (URI →
//! open-file lookup → parse-tree bracket walk → matching-opener policy →
//! wire edit). The expected ranges are derived from `DOC` by the same
//! `SourceMap` mapping the request path uses, so the assertions cannot
//! silently drift from the fixture.

use crate::database::{AnalysisDatabase, FileMap, SourceMap};
use crate::on_type_formatting::on_type_formatting;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
use lsp_types::{
    FormattingOptions, Position, TextDocumentIdentifier, TextDocumentPositionParams, Uri,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

/// The on-type scenario: the editor's auto-indent guessed 12 spaces for
/// the inner closer and 4 for the outer one; both are wrong. The engine
/// must dedent each to its opener line's indent (four spaces for the
/// `if` block, the empty string for the `fn` block).
///
/// ```text
/// fn f() {          <- opener at byte 7
///     if x {        <- opener at byte 18
///         0
///             }     <- typed `}` at byte 42  (run 30..42)
///     }             <- typed `}` at byte 48  (run 44..48)
/// ```
const DOC: &str = "fn f() {\n    if x {\n        0\n            }\n    }\n";

/// Byte offset of the first `needle` in `src`.
fn offset_of(src: &str, needle: &str) -> usize {
    src.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

/// Populate the database exactly like the analysis driver does: file map,
/// source map, and the parse tree (green node) for `src` at `path`.
fn populated_db(src: &str, path: &PathBuf) -> (AnalysisDatabase, FileMap, Uri, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let file_id = file_map.get_or_create(path);
    db.file_map.write().get_or_create(path);
    db.source_maps
        .write()
        .insert(file_id, SourceMap::new(file_id, src.to_string()));
    let green = parse_to_syntax(src, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);
    (db, file_map, uri_for(path), file_id)
}

/// Params the router registers for: `position` is *after* the typed
/// character, and `ch` is the character itself.
fn params_for(
    uri: &Uri,
    line: u32,
    character: u32,
    ch: &str,
) -> lsp_types::DocumentOnTypeFormattingParams {
    lsp_types::DocumentOnTypeFormattingParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        ch: ch.to_string(),
        options: FormattingOptions::default(),
    }
}

/// The inner `}` (typed too deep at 12 spaces) dedents to the `if` line's
/// indent: one `TextEdit` replacing the whitespace run with `    `.
#[test]
fn brace_closer_dedents_to_its_opener_line_indent() {
    let path = PathBuf::from("/test/on_type.gly");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &path);
    // The typed `}` sits at byte 42 (line 3, column 12 — after a 12-space
    // auto-indent run); the reported position is *after* it: (3, 13).
    let typed_char = offset_of(DOC, "            }") + 12;
    let sm = SourceMap::new(FileId::from_usize(0), DOC.to_string());
    let ((line, col), _) = sm.span_to_position(typed_char, typed_char).unwrap();

    let edits = on_type_formatting(
        &db,
        &file_map,
        &params_for(&uri, line as u32, col as u32 + 1, "}"),
    )
    .expect("a dedent is due");
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    // The replaced run is the line's leading whitespace (12 spaces); the
    // replacement is the opener line's indent (4 spaces).
    assert_eq!(edit.range.start.line, line as u32);
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.character, 12);
    assert_eq!(edit.new_text, "    ");
}

/// The outer `}` dedents to the `fn` line's indent — the empty string.
#[test]
fn outer_brace_dedents_to_the_empty_fn_indent() {
    let path = PathBuf::from("/test/on_type.gly");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &path);
    // The outer `}` sits at byte 48 (line 4, column 4); position (4, 5).
    let outer = offset_of(DOC, "\n    }\n") + 5;
    let sm = SourceMap::new(FileId::from_usize(0), DOC.to_string());
    let ((line, col), _) = sm.span_to_position(outer, outer).unwrap();
    assert_eq!((line, col), (4, 4), "fixture drifted");

    let edits = on_type_formatting(
        &db,
        &file_map,
        &params_for(&uri, line as u32, col as u32 + 1, "}"),
    )
    .expect("a dedent is due");
    let edit = &edits[0];
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.character, 4);
    assert_eq!(edit.new_text, "", "the fn body's closer sits at column 0");
}

/// A non-closer character (`{` was typed) answers nothing: the capability
/// fixes closers only.
#[test]
fn opener_character_answers_none() {
    let path = PathBuf::from("/test/on_type.gly");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &path);
    // Any position works — the character gate fires first.
    let edits = on_type_formatting(&db, &file_map, &params_for(&uri, 0, 8, "{"));
    assert!(edits.is_none());
}

/// The reported `ch` disagrees with the text before the position: honest
/// `None` (the client's buffer state and the server's text are out of
/// sync — guessing would rewrite a line the user did not type).
#[test]
fn character_text_mismatch_answers_none() {
    let path = PathBuf::from("/test/on_type.gly");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &path);
    let edits = on_type_formatting(&db, &file_map, &params_for(&uri, 3, 13, "x"));
    assert!(edits.is_none());
}

/// A document the server does not know answers `null`.
#[test]
fn unopened_document_answers_none() {
    let (db, file_map, _uri, _file_id) = populated_db(DOC, &PathBuf::from("/test/on_type.gly"));
    let other = PathBuf::from("/test/not_open.gly");
    let edits = on_type_formatting(&db, &file_map, &params_for(&uri_for(&other), 0, 1, "}"));
    assert!(edits.is_none());
}
