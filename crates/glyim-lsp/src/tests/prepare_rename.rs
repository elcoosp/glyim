//! Policy-level tests for the prepare-rename decision (LSP-PREPARE-RENAME).
//!
//! Like the highlight policy, the prepare decision reads occurrences
//! through an in-memory [`OccurrenceSource`] mock — no database, no locks.
//! The database-backed path (cursor resolution, exact extents, wire type)
//! is covered in `prepare_rename_tests`.

use crate::document_highlight::{OccurrenceSource, ReferenceGraphSource};
use crate::prepare_rename::rename_target;
use crate::reference_graph::{AccessKind, Reference, ReferenceGraph, ReferenceKind};
use crate::tests::helpers::make_span;
use glyim_span::FileId;
use glyim_span::Span;

/// In-memory occurrence store (same double the highlight tests use).
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

fn graphed(name: &str) -> MockSource {
    let mut src = MockSource::default();
    src.add(
        name,
        Reference {
            file_id: FileId::from_usize(1),
            span: make_span(FileId::from_usize(1), 3, 6),
            is_definition: true,
            kind: ReferenceKind::Definition,
            access: AccessKind::Read,
            def_id: None,
            owner_item_id: None,
            is_item_level: true,
        },
    );
    src
}

#[test]
fn known_identifier_yields_its_extent_and_placeholder() {
    let src = graphed("foo");
    let target = rename_target(&src, "foo", make_span(FileId::from_usize(1), 3, 6))
        .expect("a graphed symbol is renameable");
    assert_eq!(target.0.lo().to_usize(), 3);
    assert_eq!(target.0.hi().to_usize(), 6);
    assert_eq!(target.1, "foo");
}

#[test]
fn unknown_identifier_is_not_renameable() {
    let src = graphed("foo");
    // `fn` is an identifier-shaped token but has no occurrences.
    assert!(rename_target(&src, "fn", make_span(FileId::from_usize(1), 0, 2)).is_none());
}

#[test]
fn empty_occurrence_store_never_marks_renameable() {
    let src = MockSource::default();
    assert!(rename_target(&src, "anything", Span::DUMMY).is_none());
}

#[test]
fn production_source_reads_the_shared_graph() {
    // The production `OccurrenceSource` impl must answer over the real
    // `ReferenceGraph` (same store rename uses).
    let mut graph = ReferenceGraph::new();
    graph.insert_test_reference(
        "foo",
        Reference {
            file_id: FileId::from_usize(1),
            span: make_span(FileId::from_usize(1), 3, 6),
            is_definition: true,
            kind: ReferenceKind::Definition,
            access: AccessKind::Read,
            def_id: None,
            owner_item_id: None,
            is_item_level: true,
        },
    );
    let src = ReferenceGraphSource::new(&graph);
    assert_eq!(src.occurrences_of("foo").len(), 1);
    assert!(src.occurrences_of("bar").is_empty());
}
