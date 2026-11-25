//! Policy-level tests for the inline-value engine (LSP-INLINE-VALUE).
//!
//! Mirrors `moniker.rs` and `type_hierarchy.rs`: the *policy* (which
//! recorded bindings answer a requested range) is pinned through an
//! in-memory [`MockSource`] implementing
//! [`crate::inline_value::InlineValueSource`], decoupled from any HIR
//! arena, interner, or lock. The HIR-backed production path and the full
//! wire request are covered end-to-end in `inline_value_tests.rs`.

use crate::inline_value::{InlineValueSource, RecordedBinding, compute_inline_values};
use glyim_span::{FileId, Span, SyntaxContext};

/// In-memory binding store: the policy-side test double. Bindings are
/// listed in insertion order; the policy applies its own overlap,
/// ordering, dedupe, and skip rules on top.
#[derive(Default)]
struct MockSource {
    bindings: Vec<RecordedBinding>,
}

impl MockSource {
    fn binding(&mut self, name: &str, lo: usize, hi: usize) {
        self.bindings.push(RecordedBinding {
            name: name.to_string(),
            span: span_of(lo, hi),
        });
    }
}

fn span_of(lo: usize, hi: usize) -> Span {
    Span::range(
        FileId::from_usize(0),
        glyim_span::ByteIdx::from_usize(lo),
        glyim_span::ByteIdx::from_usize(hi),
        SyntaxContext::ROOT,
    )
}

impl InlineValueSource for MockSource {
    fn recorded_bindings(&self) -> Vec<RecordedBinding> {
        self.bindings.clone()
    }
}

/// Bindings overlapping the requested range answer one variable-lookup
/// fact each, in (span start) order regardless of the source's walk
/// order — a debugger frame shows them top-down.
#[test]
fn overlapping_bindings_answer_in_span_order() {
    let mut mock = MockSource::default();
    mock.binding("late", 40, 44);
    mock.binding("early", 10, 15);
    let facts = compute_inline_values(&mock, 8, 50);
    let names: Vec<&str> = facts.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["early", "late"]);
    assert_eq!(facts[0].span.lo().to_usize(), 10);
}

/// A binding outside the requested range answers nothing — the request
/// names the range, and facts outside it would be guesses.
#[test]
fn bindings_outside_the_range_answer_nothing() {
    let mut mock = MockSource::default();
    mock.binding("before", 0, 5);
    mock.binding("after", 100, 105);
    assert!(compute_inline_values(&mock, 10, 50).is_empty());
}

/// A binding merely touching the range's edge does not overlap it: the
/// overlap is strict, so a half-open range never claims a binding that
/// ends exactly where the range starts.
#[test]
fn edge_touching_bindings_do_not_overlap() {
    let mut mock = MockSource::default();
    mock.binding("ends_at_start", 0, 10);
    mock.binding("starts_at_end", 50, 60);
    assert!(compute_inline_values(&mock, 10, 50).is_empty());
    mock.binding("inside", 20, 30);
    let facts = compute_inline_values(&mock, 10, 50);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].name, "inside");
}

/// A zero-width recorded binding (the index records pattern bindings
/// without usable extents today) cannot be shown as a variable lookup:
/// the walk skips it rather than answering a location-less name.
#[test]
fn zero_width_bindings_are_skipped() {
    let mut mock = MockSource::default();
    mock.binding("ghost", 44, 44);
    mock.binding("real", 40, 44);
    let facts = compute_inline_values(&mock, 0, 100);
    let names: Vec<&str> = facts.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["real"]);
}

/// A binding whose name is empty names no variable and is skipped.
#[test]
fn empty_names_are_skipped() {
    let mut mock = MockSource::default();
    mock.binding("", 10, 14);
    mock.binding("real", 20, 24);
    let facts = compute_inline_values(&mock, 0, 100);
    let names: Vec<&str> = facts.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["real"]);
}

/// Duplicate recordings (same name at the same extent) collapse — two
/// bodies recording the same binding must not double the answer.
#[test]
fn duplicates_collapse() {
    let mut mock = MockSource::default();
    mock.binding("p", 10, 11);
    mock.binding("p", 10, 11);
    let facts = compute_inline_values(&mock, 0, 100);
    assert_eq!(facts.len(), 1);
}

/// The fact-to-wire mapping the entry point performs: every fact becomes
/// a variable-lookup inline value at the binding's extent, and the wire
/// range maps the recorded span exactly. Empty facts answer no wire list.
#[test]
fn facts_map_to_the_wire_contract() {
    let fact = crate::inline_value::InlineValueFact {
        name: "p".to_string(),
        span: span_of(10, 11),
    };
    let wire = crate::inline_value::wire_inline_value(&fact, &|lo, hi| Some(((0, lo), (0, hi))));
    let Some(lsp_types::InlineValue::VariableLookup(lookup)) = wire else {
        panic!("the fact maps to a variable lookup");
    };
    assert_eq!(lookup.variable_name.as_deref(), Some("p"));
    assert_eq!(lookup.range.start.character, 10);
    assert_eq!(lookup.range.end.character, 11);
}
