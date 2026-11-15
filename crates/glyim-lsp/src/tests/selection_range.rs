//! Policy-level tests for the selection-range engine (LSP-SELECTION).
//!
//! Ancestor chains are fed through an in-memory [`MockSource`] implementing
//! [`AncestorSpanSource`], not the live `AnalysisDatabase`, which is the
//! point of the trait seam: the selection *policy* (per-cursor chain
//! normalization — duplicate collapse, containment validation — and the
//! 1:1 offsets-to-results response shape) is verified decoupled from any
//! parse tree, lock, or source-map machinery. The parse-tree-backed path
//! (real rowan CST, `SourceMap` position math, wire linked list) is
//! covered in `selection_range_tests`.

use crate::selection_range::{AncestorSpanSource, compute_selection_chains};
use crate::tests::helpers::make_span;
use glyim_span::FileId;

/// In-memory ancestor store: the policy-side test double. Maps a cursor
/// offset to the *raw* ancestor chain the production `ParseTreeSource`
/// would return (innermost first, possibly with consecutive duplicates).
#[derive(Default)]
struct MockSource {
    entries: Vec<(usize, Vec<glyim_span::Span>)>,
}

impl MockSource {
    fn add(&mut self, offset: usize, chain: &[(usize, usize)]) {
        let spans: Vec<glyim_span::Span> = chain
            .iter()
            .map(|&(lo, hi)| make_span(FileId::from_raw(1), lo, hi))
            .collect();
        self.entries.push((offset, spans));
    }
}

impl AncestorSpanSource for MockSource {
    fn ancestors_at(&self, offset: usize) -> Option<Vec<glyim_span::Span>> {
        self.entries
            .iter()
            .find(|(off, _)| *off == offset)
            .map(|(_, chain)| chain.clone())
    }
}

#[test]
fn chain_stays_innermost_first_root_last() {
    // `let x = 1;` with the cursor on `x` (offset 5): token 5..6, the let
    // statement 0..11, the file root 0..11.
    let mut src = MockSource::default();
    src.add(5, &[(5, 6), (0, 11), (0, 11)]);

    let chains = compute_selection_chains(&src, &[5]);

    assert_eq!(chains.len(), 1, "one result per position");
    let chain = chains[0].as_ref().expect("offset resolves");
    assert_eq!(chain.len(), 2, "the duplicate root span is collapsed");
    assert_eq!(
        chain[0].lo.to_usize(),
        5,
        "innermost (the token) comes first"
    );
    assert_eq!(chain[1].lo.to_usize(), 0, "root comes last");
}

#[test]
fn consecutive_duplicate_spans_are_collapsed() {
    // Nested wrapper nodes with identical extents (a node and its only
    // child spanning the same range) must not repeat on the wire: the
    // user would have to expand selection twice with no visual change.
    let mut src = MockSource::default();
    src.add(3, &[(3, 9), (3, 9), (3, 9), (1, 12)]);

    let chains = compute_selection_chains(&src, &[3]);

    let chain = chains[0].as_ref().expect("offset resolves");
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].lo.to_usize(), 3);
    assert_eq!(chain[1].lo.to_usize(), 1);
}

#[test]
fn offset_outside_the_first_span_is_none() {
    // The LSP spec pins result[i].range to contain positions[i]; a source
    // that answers a chain starting *after* the cursor violates the
    // contract, so the policy rejects it instead of forwarding it.
    let mut src = MockSource::default();
    src.add(2, &[(5, 8), (0, 12)]);

    let chains = compute_selection_chains(&src, &[2]);

    assert!(chains[0].is_none(), "non-containing chain is rejected");
}

#[test]
fn offsets_map_one_to_one_in_order() {
    // Two cursors, one resolvable and one unknown to the source: the
    // response keeps the request order and shape (per-position Option).
    let mut src = MockSource::default();
    src.add(4, &[(4, 5), (0, 10)]);

    let chains = compute_selection_chains(&src, &[4, 50]);

    assert_eq!(chains.len(), 2);
    assert!(chains[0].is_some(), "known offset resolves");
    assert!(chains[1].is_none(), "unknown offset misses");
}

#[test]
fn empty_position_list_yields_empty_response() {
    let src = MockSource::default();
    let chains = compute_selection_chains(&src, &[]);
    assert!(chains.is_empty());
}

#[test]
fn end_of_line_cursor_is_contained_by_inclusive_end() {
    // A cursor parked exactly at a token's *end* (Between its token and
    // the next) still selects that token: containment is inclusive on
    // both ends, matching the spec's "contained in" reading for a range
    // the server itself derived from the token under the cursor.
    let mut src = MockSource::default();
    src.add(6, &[(5, 6), (0, 11)]);

    let chains = compute_selection_chains(&src, &[6]);

    assert!(chains[0].is_some(), "cursor at token end resolves");
}
