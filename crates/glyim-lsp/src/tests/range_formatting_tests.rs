//! Database-backed tests for the range-formatting request
//! (LSP-RANGE-FORMAT).
//!
//! Mirrors `formatting_tests.rs` and `inline_value_tests.rs`: the
//! [`crate::tests::test_utils::setup_test_db`] fixture populates the
//! database exactly like the driver does (the range-formatting engine
//! reads the stored document text only — no HIR or typeck pass is on its
//! path), and the public `range_formatting` entry point is exercised
//! end-to-end (URI → open-file lookup → range offsets → the claimed-line
//! policy over the stored text → the wire `TextEdit`).

use crate::range_formatting::range_formatting;
use crate::tests::test_utils::setup_test_db;
use lsp_types::request::RangeFormatting;
use lsp_types::{DocumentRangeFormattingParams, Position, Range, TextDocumentIdentifier};
use std::str::FromStr;

/// One compressed document: the shapes a selection touches — a top-level
/// statement, a compressed nested block, an already-formatted line, and
/// the closing brace.
const DOC: &str = "let a=1;\nfn main() {\n    {let b=2;}\n    let c = 3;\n}\n";

/// Params over a line range of `DOC` (start line inclusive, end line
/// exclusive), the way an editor sends a selection.
fn params_for(
    uri: lsp_types::Uri,
    start_line: u32,
    end_line: u32,
) -> DocumentRangeFormattingParams {
    DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range {
            start: Position {
                line: start_line,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: 0,
            },
        },
        options: lsp_types::FormattingOptions::default(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    }
}

/// A range over the compressed nested block claims exactly that line and
/// re-indents it at the depth the unselected prefix leaves, with byte-exact
/// edit bounds (line 2 start .. line 3 start).
#[test]
fn a_nested_range_claims_exactly_its_lines() {
    let (db, file_map, uri, _file_id) = setup_test_db(DOC, "/test/range_fmt_nested.g");
    let params = params_for(uri, 2, 3);
    let edits = range_formatting(&db, &file_map, &params).expect("line 2 is compressed");
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.range.start.line, 2);
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.line, 3);
    assert_eq!(edit.range.end.character, 0);
    // The compressed `{let b=2;}` block expands to its own lines at the
    // depth the unselected prefix leaves (the `fn` block), and the edit's
    // new_text replaces the whole claimed line span.
    assert_eq!(edit.new_text, "    {\n        let b=2;\n    }\n");
}

/// Splicing the edit back preserves every unselected line byte-for-byte
/// and re-indents the claimed line — the response an editor applies
/// verbatim.
#[test]
fn the_edit_splices_back_without_touching_the_suffix() {
    let (db, file_map, uri, _file_id) = setup_test_db(DOC, "/test/range_fmt_splice.g");
    let params = params_for(uri, 2, 3);
    let edits = range_formatting(&db, &file_map, &params).expect("line 2 is compressed");
    let edit = &edits[0];
    let mut spliced = DOC.to_string();
    // The wire range maps to the same claimed span the policy selected;
    // splice by line arithmetic (the edit covers whole lines).
    let lo = DOC
        .split_inclusive('\n')
        .take(edit.range.start.line as usize)
        .map(str::len)
        .sum::<usize>();
    let hi = lo
        + DOC[lo..]
            .split_inclusive('\n')
            .take((edit.range.end.line - edit.range.start.line) as usize)
            .map(str::len)
            .sum::<usize>();
    spliced.replace_range(lo..hi, &edit.new_text);
    let expected = "let a=1;\nfn main() {\n    {\n        let b=2;\n    }\n    let c = 3;\n}\n";
    assert_eq!(spliced, expected);
}

/// A range over an already-formatted line answers `null` — no edit churn
/// on the request path.
#[test]
fn an_already_formatted_range_answers_null() {
    let (db, file_map, uri, _file_id) = setup_test_db(DOC, "/test/range_fmt_clean.g");
    let params = params_for(uri, 3, 4);
    assert!(range_formatting(&db, &file_map, &params).is_none());
}

/// The whole-document range agrees with the full-document formatter's
/// `new_text` (one formatter, two entry shapes): the range response over
/// `[0, line_count)` carries the same formatted text the full request
/// would return.
#[test]
fn the_whole_document_range_agrees_with_the_full_formatter() {
    let (db, file_map, uri, _file_id) = setup_test_db(DOC, "/test/range_fmt_whole.g");
    let params = params_for(uri, 0, DOC.lines().count() as u32);
    let edits = range_formatting(&db, &file_map, &params).expect("the document is compressed");
    assert_eq!(edits.len(), 1);
    let full = crate::formatting::format_document(
        &db,
        &lsp_types::DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: params.text_document.uri.clone(),
            },
            options: lsp_types::FormattingOptions::default(),
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        },
    )
    .expect("the full formatter formats the document");
    assert_eq!(edits[0].new_text, full[0].new_text);
}

/// The wire contract the engine implements: the lsp-types request type's
/// METHOD string is pinned so an lsp-types upgrade that reshapes the
/// endpoint surfaces here, not on a client's screen.
#[test]
fn wire_method_is_pinned() {
    use lsp_types::request::Request;
    assert_eq!(RangeFormatting::METHOD, "textDocument/rangeFormatting");
}

/// A document the server does not know (not open, never analysed)
/// answers `null`.
#[test]
fn an_unopened_document_answers_null() {
    let (db, file_map, _uri, _file_id) = setup_test_db(DOC, "/test/never_opened_rf.g");
    let params = params_for(
        lsp_types::Uri::from_str("file:///test/ghost.g").unwrap(),
        0,
        2,
    );
    assert!(range_formatting(&db, &file_map, &params).is_none());
}
