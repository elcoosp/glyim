//! Database-backed tests for the color requests (LSP-COLOR).
//!
//! Mirrors `document_link_tests.rs`: the parse tree is stored in the
//! `AnalysisDatabase` exactly as the analysis driver stores it, a
//! `SourceMap` carries the document text, and both public entry points are
//! exercised end-to-end (`documentColor`: URI → open-file lookup →
//! parse-tree literal walk → color-site policy → wire colors;
//! `colorPresentation`: the same, plus the range→site validation → the
//! spelling edit). The expected positions are derived from `DOC` by token
//! search + the same `SourceMap` mapping the request path uses, so the
//! assertions cannot silently drift from the fixture.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::document_color::{color_presentations, document_colors};
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;
use lsp_types::{
    Color, ColorPresentationParams, DocumentColorParams, PartialResultParams,
    TextDocumentIdentifier, Uri, WorkDoneProgressParams,
};
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn uri_for(path: &PathBuf) -> Uri {
    Uri::from_str(Url::from_file_path(path).unwrap().as_ref()).unwrap()
}

fn color_params_for(uri: Uri) -> DocumentColorParams {
    DocumentColorParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn presentation_params_for(
    uri: Uri,
    color: Color,
    range: lsp_types::Range,
) -> ColorPresentationParams {
    ColorPresentationParams {
        text_document: TextDocumentIdentifier { uri },
        color,
        range,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

/// Two color sites (a full six-digit and a shorthand) around one
/// non-color string: the swatches answer in source order and the prose
/// string answers nothing.
const DOC: &str = "fn main() -> i32 {\n    let fill = \"#ff00aa\";\n    let name = \"not-a-color\";\n    let theme = \"#f0a\";\n    0\n}\n";

/// A non-ASCII identifier before the first color: the payload's wire
/// range must run through UTF-16 columns (INF-11), which the byte-extent
/// → position mapping has to translate.
const UNICODE_DOC: &str = "fn ünïcödé() -> i32 {\n    let fill = \"#ff00aa\";\n    0\n}\n";

/// Byte offset of the first `needle` in `src`.
fn offset_of(src: &str, needle: &str) -> usize {
    src.find(needle)
        .unwrap_or_else(|| panic!("fixture drifted: `{needle}` not in DOC"))
}

/// Map byte offsets to the exact wire positions the engine must answer
/// with: the same `SourceMap` line/UTF-16-column mapping the request path
/// uses, so the expectations cannot drift from the mapping under test.
fn lsp_range(source: &str, lo: usize, hi: usize) -> lsp_types::Range {
    let sm = crate::database::SourceMap::new(FileId::from_raw(0), source.to_string());
    let ((start_line, start_col), (end_line, end_col)) = sm
        .span_to_position(lo, hi)
        .unwrap_or_else(|| panic!("fixture extent {lo}..{hi} is not char-boundary safe"));
    lsp_types::Range {
        start: lsp_types::Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: lsp_types::Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    }
}

/// The payload extent of the first color literal in `src` (quotes
/// excluded — the extent the swatch and the presentation edit claim).
fn payload_extent(src: &str, literal: &str) -> (usize, usize) {
    let literal_lo = offset_of(src, literal);
    (literal_lo + 1, literal_lo + literal.len() - 1)
}

/// Populate the database exactly like the analysis driver does: file map,
/// source map, and the parse tree (green node) for `src` at `path`.
fn populated_db(src: &str, path: &PathBuf) -> (AnalysisDatabase, FileMap, Uri, FileId) {
    let db = AnalysisDatabase::new();
    let mut file_map = FileMap::new();
    let file_id = file_map.get_or_create(path);
    db.file_map.write().get_or_create(path);
    db.source_maps.write().insert(
        file_id,
        crate::database::SourceMap::new(file_id, src.to_string()),
    );
    let green = parse_to_syntax(src, file_id).green_node;
    db.syntaxes.write().insert(file_id, green);
    let uri = uri_for(path);
    (db, file_map, uri, file_id)
}

#[test]
fn colors_answer_in_source_order_with_payload_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);

    let colors = document_colors(&db, &file_map, &color_params_for(uri)).expect("colors expected");

    assert_eq!(colors.len(), 2, "one swatch per color literal");
    let (fill_lo, fill_hi) = payload_extent(DOC, "\"#ff00aa\"");
    let (theme_lo, theme_hi) = payload_extent(DOC, "\"#f0a\"");
    assert_eq!(colors[0].range, lsp_range(DOC, fill_lo, fill_hi));
    assert_eq!(colors[0].color.red, 1.0);
    assert_eq!(colors[0].color.blue, 170.0 / 255.0);
    assert_eq!(colors[0].color.alpha, 1.0);
    assert_eq!(colors[1].range, lsp_range(DOC, theme_lo, theme_hi));
    assert_eq!(colors[1].color.green, 0.0, "the shorthand expands over 15");
}

#[test]
fn positions_run_through_utf16_columns() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("unicode.g");
    let (db, file_map, uri, _file_id) = populated_db(UNICODE_DOC, &doc_path);
    let colors = document_colors(&db, &file_map, &color_params_for(uri)).expect("colors expected");
    assert_eq!(colors.len(), 1);
    let (lo, hi) = payload_extent(UNICODE_DOC, "\"#ff00aa\"");
    assert_eq!(colors[0].range, lsp_range(UNICODE_DOC, lo, hi));
    assert_eq!(colors[0].range.start.line, 1);
}

#[test]
fn unopened_document_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, _uri, _file_id) = populated_db(DOC, &doc_path);
    // A URI that was never opened: no file-map entry, so no answer.
    let other = uri_for(&dir.path().join("never-opened.g"));
    assert!(document_colors(&db, &file_map, &color_params_for(other)).is_none());
}

#[test]
fn a_color_free_document_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("plain.g");
    let src = "fn main() -> i32 {\n    let name = \"not-a-color\";\n    0\n}\n";
    let (db, file_map, uri, _file_id) = populated_db(src, &doc_path);
    assert!(document_colors(&db, &file_map, &color_params_for(uri)).is_none());
}

/// The picker echo: presentations answer exactly one spelling for the
/// requested color, and the edit replaces the reported payload extent —
/// applying it re-spells the site.
#[test]
fn presentations_answer_one_edit_over_the_reported_range() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);

    let (lo, hi) = payload_extent(DOC, "\"#ff00aa\"");
    let requested = Color {
        red: 63.0 / 255.0,
        green: 167.0 / 255.0,
        blue: 214.0 / 255.0,
        alpha: 1.0,
    };
    let params = presentation_params_for(uri, requested, lsp_range(DOC, lo, hi));
    let presentations = color_presentations(&db, &file_map, &params)
        .expect("a recorded site expects presentations");

    assert_eq!(presentations.len(), 1, "one notation, one presentation");
    let presentation = &presentations[0];
    assert_eq!(presentation.label, "#3fa7d6");
    let edit = presentation
        .text_edit
        .as_ref()
        .expect("the splice is carried as a TextEdit");
    assert_eq!(edit.range, lsp_range(DOC, lo, hi));
    assert_eq!(edit.new_text, "#3fa7d6");

    // Applying the answered edit re-spells the site in place.
    let mut doc = DOC.to_string();
    let sm = crate::database::SourceMap::new(FileId::from_raw(0), DOC.to_string());
    let start = sm
        .line_col_to_offset(
            edit.range.start.line as usize,
            edit.range.start.character as usize,
        )
        .unwrap();
    let end = sm
        .line_col_to_offset(
            edit.range.end.line as usize,
            edit.range.end.character as usize,
        )
        .unwrap();
    doc.replace_range(start..end, &edit.new_text);
    assert!(doc.contains("\"#3fa7d6\""), "the site re-spelled: {doc:?}");
}

/// A semi-transparent request spells the eight-digit form; the edit still
/// covers exactly the payload.
#[test]
fn a_transparent_request_spells_the_eight_digit_form() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);

    let (lo, hi) = payload_extent(DOC, "\"#f0a\"");
    let requested = Color {
        red: 1.0,
        green: 0.0,
        blue: 170.0 / 255.0,
        alpha: 128.0 / 255.0,
    };
    let params = presentation_params_for(uri, requested, lsp_range(DOC, lo, hi));
    let presentations = color_presentations(&db, &file_map, &params).expect("presentations");
    assert_eq!(presentations[0].label, "#ff00aa80");
    assert_eq!(
        presentations[0].text_edit.as_ref().unwrap().range,
        lsp_range(DOC, lo, hi)
    );
}

/// A range no recorded site occupies answers `null` — a presentation for
/// arbitrary text would splice hex over prose, and that is never
/// offered.
#[test]
fn a_presentation_over_unrecorded_text_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, uri, _file_id) = populated_db(DOC, &doc_path);

    // The non-color string's quoted extent: a real range, but not a site.
    let (lo, hi) = payload_extent(DOC, "\"not-a-color\"");
    let params = presentation_params_for(
        uri,
        Color {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        },
        lsp_range(DOC, lo, hi),
    );
    assert!(color_presentations(&db, &file_map, &params).is_none());
}

#[test]
fn unopened_document_presentation_answers_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc_path = dir.path().join("doc.g");
    let (db, file_map, _uri, _file_id) = populated_db(DOC, &doc_path);
    let other = uri_for(&dir.path().join("never-opened.g"));
    let params = presentation_params_for(
        other,
        Color {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        },
        lsp_range(DOC, 0, 4),
    );
    assert!(color_presentations(&db, &file_map, &params).is_none());
}

/// The wire contracts the engine implements: the lsp-types request METHOD
/// strings are pinned so an lsp-types upgrade that reshapes the endpoints
/// surfaces here, not on a client's screen.
#[test]
fn wire_methods_are_pinned() {
    use lsp_types::request::{ColorPresentationRequest, DocumentColor, Request};
    assert_eq!(DocumentColor::METHOD, "textDocument/documentColor");
    assert_eq!(
        ColorPresentationRequest::METHOD,
        "textDocument/colorPresentation"
    );
}
