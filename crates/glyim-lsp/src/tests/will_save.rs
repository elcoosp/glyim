//! Policy-level tests for the save-time edit engine (LSP-WILL-SAVE).
//!
//! Mirrors `range_formatting.rs` and `inline_value.rs`: the *policy*
//! (which edits bring the stored document to its saved-form state) is
//! pinned through an in-memory [`MockSource`] implementing
//! [`crate::will_save::SaveFormatSource`], decoupled from any database,
//! source map, or lock. The stored-text production path and the full
//! wire request are covered end-to-end in `will_save_tests.rs`.

use crate::will_save::{SaveEdit, SaveFormatSource, compute_save_edits};

/// In-memory document store: the policy-side test double. The policy
/// reads the whole stored text through the seam and owns every split,
/// diff, and byte-extent decision on top.
struct MockSource {
    text: String,
}

impl SaveFormatSource for MockSource {
    fn document_text(&self) -> &str {
        &self.text
    }
}

/// A document the formatter considers already formatted: feeding it back
/// through the shared core is a fixpoint, so a save must answer no edit
/// (the same no-churn contract the full-document formatter honours).
#[test]
fn an_already_formatted_document_answers_no_edit() {
    let doc = "fn main() -> i32 {\n    let p = 5;\n    p\n}\n";
    let mock = MockSource {
        text: doc.to_string(),
    };
    assert!(compute_save_edits(&mock).is_none());
}

/// An empty document has nothing to save-format either: the formatter's
/// fixpoint of `""` is `""`, and the request answers no edit rather than
/// an edit with no effect.
#[test]
fn an_empty_document_answers_no_edit() {
    let mock = MockSource {
        text: String::new(),
    };
    assert!(compute_save_edits(&mock).is_none());
}

/// One mis-indented line in an otherwise formatted document answers
/// exactly one edit, and its byte span covers only the changed line —
/// the untouched head and tail stay outside the edit so the editor's
/// cursor, folding state, and undo granularity outside the changed
/// region survive the save.
#[test]
fn a_single_unformatted_line_answers_one_edit_over_that_line() {
    let mut doc = String::from("fn main() -> i32 {\n");
    doc.push_str("    let head = 1;\n");
    doc.push_str("let broken = 2;\n"); // the mis-indented middle line
    doc.push_str("    let tail = 3;\n");
    doc.push_str("}\n");
    let mock = MockSource { text: doc.clone() };

    let edit = compute_save_edits(&mock).expect("the broken line needs an edit");
    // "fn main() -> i32 {\n" is 19 bytes and "    let head = 1;\n" is
    // 18, so the mis-indented line spans bytes [37, 53) — head and tail
    // stay outside the span.
    assert_eq!(edit.start, 37, "the edit starts at the broken line");
    assert_eq!(edit.end, 53, "the edit ends at the broken line's newline");
    assert_eq!(edit.new_text, "    let broken = 2;\n");
}

/// The save contract in one round trip: applying the answered edit to
/// the stored text reproduces byte-for-byte what the shared formatter
/// core would answer for the whole document — the save shape and the
/// `textDocument/formatting` shape are one formatter by construction.
#[test]
fn applying_the_edit_reproduces_the_document_formatter_output() {
    let mut doc = String::from("fn outer() -> i32 {\n");
    doc.push_str("    let formatted_head = 1;\n");
    doc.push_str("{\nlet nested = 2;\n}\n"); // an unformatted middle block
    doc.push_str("    let formatted_tail = 3;\n");
    doc.push_str("}\n");
    let mock = MockSource { text: doc.clone() };

    let edit = compute_save_edits(&mock).expect("the middle block needs an edit");
    let mut saved = String::with_capacity(doc.len());
    saved.push_str(&doc[..edit.start]);
    saved.push_str(&edit.new_text);
    saved.push_str(&doc[edit.end..]);

    let formatted = crate::formatting::format_source(&doc, 0);
    assert_eq!(
        saved, formatted,
        "the save edits compose to the formatted document"
    );
}

/// A document stored without its trailing newline still saves to the
/// formatter's newline-terminated shape: the missing terminator is a
/// zero-width insertion edit at EOF, never a rewrite of the whole line.
#[test]
fn a_missing_trailing_newline_answers_an_insertion_at_eof() {
    let mock = MockSource {
        text: "let x = 1;".to_string(),
    };
    let edit = compute_save_edits(&mock).expect("the terminator is missing");
    assert_eq!(edit.start, 10, "the insertion sits at EOF");
    assert_eq!(edit.end, 10, "an insertion claims no bytes");
    assert_eq!(edit.new_text, "\n");
}

/// Whitespace-only drift trims to a replacement over the drifted line:
/// the trailing space on one line answers a replacement whose
/// `new_text` is the trimmed form, and nothing outside the drifted line
/// is touched.
#[test]
fn a_trailing_space_answers_a_replacement_over_one_line() {
    let mock = MockSource {
        text: "let a = 1 \nlet b = 2;\n".to_string(),
    };
    let edit = compute_save_edits(&mock).expect("the trailing space drifts");
    assert_eq!(edit.start, 0);
    assert_eq!(
        edit.end, 11,
        "the edit covers the drifted line including its newline"
    );
    assert_eq!(edit.new_text, "let a = 1\n");
}

/// The edit-to-wire mapping the entry point performs: the byte span maps
/// through the injected span→position mapping into the `TextEdit` range,
/// and the replacement text carries over unchanged. An unmappable span
/// answers `None` — never a guessed position.
#[test]
fn the_edit_maps_to_the_wire_contract() {
    let edit = SaveEdit {
        start: 4,
        end: 9,
        new_text: "    let p = 5;\n".to_string(),
    };
    let wire = crate::will_save::wire_save_edit(&edit, &|lo, hi| Some(((0, lo), (0, hi))));
    let Some(text_edit) = wire else {
        panic!("a mappable span maps to a TextEdit");
    };
    assert_eq!(text_edit.range.start.character, 4);
    assert_eq!(text_edit.range.end.character, 9);
    assert_eq!(text_edit.new_text, "    let p = 5;\n");

    let unmappable = crate::will_save::wire_save_edit(&edit, &|_, _| None);
    assert!(unmappable.is_none(), "an unmappable span answers None");
}
