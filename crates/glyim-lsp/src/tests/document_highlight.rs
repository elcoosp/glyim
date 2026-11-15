//! Policy-level tests for the document-highlight engine (LSP-HIGHLIGHT).
//!
//! Occurrences are fed through an in-memory [`OccurrenceSource`] mock, not
//! the live `AnalysisDatabase`, which is the point of the trait seam: the
//! highlight *policy* (cursor resolution, scope filtering, kind
//! classification) is verified decoupled from any database, lock, or
//! source-map machinery. The database-backed path (real `ReferenceGraph`,
//! `SourceMap` position math, wire types) is covered in
//! `document_highlight_tests`.

use crate::document_highlight::{HighlightKind, OccurrenceSource, compute_highlights};
use crate::reference_graph::{AccessKind, Reference, ReferenceKind};
use crate::tests::helpers::make_span;
use glyim_span::{FileId, Span};

/// In-memory occurrence store: the policy-side test double.
#[derive(Default)]
struct MockSource {
    entries: Vec<(String, Reference)>,
}

impl MockSource {
    fn add(&mut self, symbol: &str, r: Reference) {
        self.entries.push((symbol.to_string(), r));
    }
}

impl OccurrenceSource for MockSource {
    fn occurrences_of(&self, symbol: &str) -> Vec<Reference> {
        self.entries
            .iter()
            .filter(|(name, _)| name == symbol)
            .map(|(_, r)| r.clone())
            .collect()
    }
}

/// The file every in-document fixture lives in.
fn main_file() -> FileId {
    FileId::from_raw(1)
}
/// A second file, used to assert cross-file occurrences are excluded.
fn other_file() -> FileId {
    FileId::from_raw(2)
}

/// Local-scoped occurrence: owner `owner`, not item-level.
fn local(file_id: FileId, lo: usize, hi: usize, owner: Option<u32>) -> Reference {
    Reference {
        file_id,
        span: make_span(file_id, lo, hi),
        is_definition: false,
        kind: ReferenceKind::Variable,
        access: AccessKind::Read,
        def_id: None,
        owner_item_id: owner,
        is_item_level: false,
    }
}

/// Definition occurrence (span of the binding site itself).
fn definition(file_id: FileId, lo: usize, hi: usize, owner: Option<u32>) -> Reference {
    Reference {
        file_id,
        span: make_span(file_id, lo, hi),
        is_definition: true,
        kind: ReferenceKind::Definition,
        access: AccessKind::Read,
        def_id: None,
        owner_item_id: owner,
        is_item_level: false,
    }
}

#[test]
fn definition_read_and_write_get_distinct_kinds() {
    // `x` defined at 6..7, written at 14..15, read at 22..23 — one function
    // (owner 7). Cursor on the read.
    let mut src = MockSource::default();
    src.add("x", definition(main_file(), 6, 7, Some(7)));
    let mut write_ref = local(main_file(), 14, 15, Some(7));
    write_ref.access = AccessKind::Write;
    src.add("x", write_ref);
    src.add("x", local(main_file(), 22, 23, Some(7)));

    let highlights = compute_highlights(&src, main_file(), 22, "x").expect("cursor is on a read");

    assert_eq!(highlights.len(), 3);
    assert_eq!(highlights[0].kind, HighlightKind::Text); // the definition
    assert_eq!(highlights[1].kind, HighlightKind::Write);
    assert_eq!(highlights[2].kind, HighlightKind::Read);
    assert_eq!(highlights[0].span.lo.to_usize(), 6);
    assert_eq!(highlights[2].span.lo.to_usize(), 22);
}

#[test]
fn scoped_occurrences_in_other_scopes_are_excluded() {
    // Two functions each hold a local `x` (owners 7 and 9). The graph is
    // name-keyed, so both scopes' occurrences come back under one name;
    // the policy must keep only the cursor's scope (T129 [LSP-5] rule).
    let mut src = MockSource::default();
    src.add("x", definition(main_file(), 6, 7, Some(7)));
    src.add("x", local(main_file(), 14, 15, Some(7)));
    src.add("x", definition(main_file(), 46, 47, Some(9)));
    src.add("x", local(main_file(), 54, 55, Some(9)));

    let highlights =
        compute_highlights(&src, main_file(), 14, "x").expect("cursor is on owner-7 read");

    assert_eq!(highlights.len(), 2, "owner-9 occurrences must be excluded");
    for h in &highlights {
        assert!(
            h.span.lo.to_usize() < 40,
            "only owner-7 (early-offset) occurrences may appear, got {:?}",
            h.span
        );
    }
}

#[test]
fn item_level_occurrences_stay_global() {
    // A free function `foo` at item level: every occurrence of the name is
    // highlighted regardless of owner (owners are irrelevant at item level).
    let mut src = MockSource::default();
    let mut def = definition(main_file(), 4, 7, None);
    def.is_item_level = true;
    def.kind = ReferenceKind::Definition;
    src.add("foo", def);
    let mut call = local(main_file(), 16, 19, None);
    call.is_item_level = true;
    call.kind = ReferenceKind::Call;
    src.add("foo", call);
    // Another item-level occurrence with a *different* owner — must stay.
    let mut cross = local(main_file(), 30, 33, Some(42));
    cross.is_item_level = true;
    src.add("foo", cross);

    let highlights =
        compute_highlights(&src, main_file(), 17, "foo").expect("cursor is on the call");

    assert_eq!(
        highlights.len(),
        3,
        "item-level names are not scope-filtered"
    );
}

#[test]
fn cursor_off_any_occurrence_is_none() {
    let mut src = MockSource::default();
    src.add("x", definition(main_file(), 6, 7, Some(7)));

    // Offset 100 lands on no recorded occurrence.
    assert!(compute_highlights(&src, main_file(), 100, "x").is_none());
}

#[test]
fn dummy_spans_are_skipped() {
    // Pattern bindings currently carry DUMMY spans (T053 [LSP-4]); a dummy
    // occurrence must never surface as a highlight range.
    let mut src = MockSource::default();
    src.add("x", definition(main_file(), 6, 7, Some(7)));
    let mut dummy = local(main_file(), 14, 15, Some(7));
    dummy.span = Span::DUMMY; // the pattern-binding shape (T053 [LSP-4])
    src.add("x", dummy);

    let highlights =
        compute_highlights(&src, main_file(), 6, "x").expect("cursor is on the definition");

    assert_eq!(highlights.len(), 1, "the DUMMY-span occurrence is skipped");
    assert_eq!(highlights[0].span.lo.to_usize(), 6);
}

#[test]
fn occurrences_in_other_files_are_not_highlighted() {
    // Document highlight is per-document by spec: occurrences of the same
    // item-level name in another open file must not leak into the response.
    let mut src = MockSource::default();
    let mut def = definition(main_file(), 4, 7, None);
    def.is_item_level = true;
    src.add("foo", def);
    let mut cross_file = local(other_file(), 10, 13, None);
    cross_file.is_item_level = true;
    src.add("foo", cross_file);

    let highlights =
        compute_highlights(&src, main_file(), 5, "foo").expect("cursor is on the definition");

    assert_eq!(
        highlights.len(),
        1,
        "only the current document's occurrences"
    );
    assert_eq!(highlights[0].span.lo.to_usize(), 4);
}

#[test]
fn write_access_on_definition_site_classifies_as_text() {
    // A definition may be recorded with a Write access (the binding
    // initializes the variable); `is_definition` dominates the access
    // classification so the binding site renders as Text, not Write.
    let mut src = MockSource::default();
    let mut def = definition(main_file(), 6, 7, Some(7));
    def.access = AccessKind::Write;
    src.add("x", def);
    src.add("x", local(main_file(), 14, 15, Some(7)));

    let highlights =
        compute_highlights(&src, main_file(), 6, "x").expect("cursor is on the definition");

    assert_eq!(highlights[0].kind, HighlightKind::Text);
}
