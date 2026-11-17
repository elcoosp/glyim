//! Policy-level tests for the range-formatting engine (LSP-RANGE-FORMAT).
//!
//! Mirrors `inline_value.rs` and `moniker.rs`: the *policy* (which whole
//! lines the requested range claims, what the seeded re-indent produces,
//! and when the honest answer is "no edit") is pinned through an in-memory
//! [`MockSource`] implementing [`crate::range_formatting::RangeFormatSource`],
//! decoupled from any database, source map, or lock. The production
//! database-backed path and the full wire request are covered end-to-end
//! in `range_formatting_tests.rs`.

use crate::range_formatting::{RangeFormatSource, compute_range_edits};

/// In-memory document store: the policy-side test double. The policy
/// expands the requested byte range to whole lines, seeds the re-indent
/// from the brace balance of the unselected prefix, and emits at most one
/// edit — all on top of whatever text this mock returns.
#[derive(Default)]
struct MockSource {
    text: String,
}

impl MockSource {
    fn with_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl RangeFormatSource for MockSource {
    fn document_text(&self) -> &str {
        &self.text
    }
}

/// Byte offset of a line start in `text` (0-indexed lines).
fn line_start(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line).map(str::len).sum()
}

/// A range over compressed lines inside a nested block reformats only the
/// claimed lines, seeded with the nesting depth the unselected prefix
/// leaves: the re-indent composes with the surrounding file instead of
/// resetting to column 0. The formatter spaces commas, never operators,
/// so `let a=1;` keeps its tight `=`.
#[test]
fn nested_range_reindents_with_the_prefix_depth() {
    // Line 0 opens a block; line 1 compresses a nested block.
    let text = "fn main() {\n    {let a=1;let b=2;}\n    let c = 3;\n}\n";
    let mock = MockSource::with_text(text);
    let lo = line_start(text, 1);
    let hi = line_start(text, 2);
    let edit = compute_range_edits(&mock, lo, hi).expect("a compressed line claims one edit");
    // The edit covers exactly line 1 (its whole line span) and carries the
    // base indent of depth 1 (the `fn` block) plus the inner block's own
    // brace on that same line seeding from the prefix depth.
    assert_eq!(edit.start, line_start(text, 1));
    assert_eq!(edit.end, line_start(text, 2));
    assert_eq!(
        edit.new_text,
        "    {\n        let a=1;\n        let b=2;\n    }\n"
    );
}

/// Lines the range does not overlap are untouched: the edit span starts at
/// the first overlapping line and ends after the last overlapping line's
/// newline, and the new text splices back so the prefix and suffix survive
/// byte-for-byte.
#[test]
fn only_the_claimed_lines_are_replaced() {
    // Line 1 is over-indented for its depth-0 surroundings — a compressed
    // line the formatter can fix without changing the line count.
    let text = "let a = 1;\n        let b = 2;\nlet c = 3;\n";
    let mock = MockSource::with_text(text);
    let lo = line_start(text, 1);
    let hi = line_start(text, 2);
    let edit = compute_range_edits(&mock, lo, hi).expect("line 1 is over-indented");
    assert_eq!(edit.start, line_start(text, 1));
    assert_eq!(edit.end, line_start(text, 2));
    assert_eq!(edit.new_text, "let b = 2;\n");
    // Splice: the formatted slice replaces [start, end), everything else
    // survives.
    let spliced: String = text[..edit.start].to_string() + &edit.new_text + &text[edit.end..];
    assert_eq!(spliced, "let a = 1;\nlet b = 2;\nlet c = 3;\n");
}

/// A range that merely touches a line's end does not claim the next line:
/// the overlap is half-open, so a selection ending at a line start never
/// reformats that line.
#[test]
fn a_range_ending_at_a_line_start_does_not_claim_that_line() {
    let text = "{let a=1;}\n{let b=2;}\n";
    let mock = MockSource::with_text(text);
    // Range over line 0 only: end byte = start of line 1.
    let edit = compute_range_edits(&mock, 0, line_start(text, 1)).expect("line 0 is compressed");
    assert_eq!(
        edit.end,
        line_start(text, 1),
        "the edit stops at line 0's newline"
    );
    // And a range starting exactly at a line start claims from there.
    let edit2 =
        compute_range_edits(&mock, line_start(text, 1), text.len()).expect("line 1 is compressed");
    assert_eq!(edit2.start, line_start(text, 1));
}

/// An already-formatted range answers `None` — no edit churn, the same
/// contract the full-document formatter honours.
#[test]
fn an_already_formatted_range_answers_none() {
    let text = "let a = 1;\nlet b = 2;\n";
    let mock = MockSource::with_text(text);
    assert!(compute_range_edits(&mock, 0, line_start(text, 1)).is_none());
}

/// An empty range claims no line: a zero-width selection has nothing to
/// reformat, so the honest answer is `None`, not a guessed line.
#[test]
fn an_empty_range_answers_none() {
    let text = "let a=1;\n";
    let mock = MockSource::with_text(text);
    assert!(compute_range_edits(&mock, 4, 4).is_none());
}

/// A reversed range is clamped to `[lo, lo)` — the same non-panicking
/// clamp the other range-consuming engines apply — and an empty result
/// answers `None`.
#[test]
fn a_reversed_range_is_clamped() {
    let text = "let a=1;\nlet b=2;\n";
    let mock = MockSource::with_text(text);
    let hi = line_start(text, 1);
    let lo = line_start(text, 2);
    assert!(compute_range_edits(&mock, lo, hi).is_none());
}

/// The final line without a trailing newline still formats: the claimed
/// span ends at the document's last byte and the formatted text terminates
/// the line without splicing in a phantom blank line.
#[test]
fn the_unterminated_final_line_formats() {
    let text = "fn main() {\nlet x=1;\n}";
    let mock = MockSource::with_text(text);
    let lo = line_start(text, 2);
    let edit = compute_range_edits(&mock, lo, text.len()).expect("line 2 is compressed");
    assert_eq!(edit.end, text.len());
    assert_eq!(edit.new_text, "}\n");
}

/// The composition contract that makes range formatting a *seam* rather
/// than a second formatter: reformatting a document range-by-range with
/// the prefix-derived base indent produces the same text reformatting the
/// whole document in one call (the shared policy is the single formatter).
#[test]
fn range_by_range_composes_with_the_whole_document() {
    let text = "fn main() {\nlet a=1;\nlet b=2;\n}\n";
    let mock = MockSource::with_text(text);
    // Whole document, one edit.
    let whole = compute_range_edits(&mock, 0, text.len()).expect("the document is compressed");
    // First range: lines 0-1 (the header + one statement), base indent 0.
    // The span claims through line 1's start, so both lines are inside
    // the claim — with the shared core idempotent for a `fn main() {`
    // header, a line-0-only claim is already formatted and answers
    // `None` (the no-churn contract), which would leave line 1 outside
    // every range and break the composition this test pins.
    let lo = line_start(text, 2);
    let first = compute_range_edits(&mock, 0, lo).expect("lines 0-1 are compressed");
    let mut spliced = text[..first.start].to_string() + &first.new_text + &text[first.end..];
    // Second range: the remaining lines, byte offsets taken from the
    // spliced state (the first edit shifted them) — exactly what a client
    // applying edits in order does. Its base indent comes from the prefix
    // the first formatted range left behind.
    let lo2 = line_start(&spliced, 2);
    let second = compute_range_edits(&MockSource::with_text(&spliced), lo2, spliced.len())
        .expect("line 2 is compressed");
    spliced = spliced[..second.start].to_string() + &second.new_text + &spliced[second.end..];
    assert_eq!(
        spliced,
        format!("{}{}", &text[..whole.start], whole.new_text)
    );
}

/// The wire mapping the entry point performs: the edit's byte extents map
/// through the injected line/column mapping, and a mapping failure (a
/// file gone from the session) yields `None` rather than a guessed
/// position.
#[test]
fn the_edit_maps_to_the_wire_contract() {
    use crate::range_formatting::wire_range_formatting;
    use lsp_types::TextEdit;
    let edit = crate::range_formatting::RangeEdit {
        start: 10,
        end: 20,
        new_text: "let a = 1;\n".to_string(),
    };
    let wire = wire_range_formatting(&edit, &|lo, hi| Some(((1, lo), (1, hi))));
    let Some(wire_edit) = wire else {
        panic!("a mappable edit maps to a TextEdit");
    };
    assert!(matches!(wire_edit, TextEdit { .. }));
    assert_eq!(wire_edit.range.start.line, 1);
    assert_eq!(wire_edit.range.start.character, 10);
    assert_eq!(wire_edit.range.end.character, 20);
    assert_eq!(wire_edit.new_text, "let a = 1;\n");
    let unmappable = wire_range_formatting(&edit, &|_, _| None);
    assert!(unmappable.is_none(), "an unmappable edit answers None");
}
