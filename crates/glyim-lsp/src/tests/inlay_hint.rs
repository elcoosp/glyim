//! Policy-level tests for the inlay-hint engine (LSP-INLAY-HINT).
//!
//! Hint selection is decided through an in-memory [`MockSource`]
//! implementing [`BindingTypeSource`], not the live `AnalysisDatabase` —
//! the point of the trait seam: the *policy* (which recorded bindings
//! become hints, and in what order) is verified decoupled from any HIR
//! arena, typeck map, or lock. The HIR-backed path (real binding spans,
//! typeck resolution, wire positions) is covered in `inlay_hint_tests`.

use crate::inlay_hint::{BindingTypeSource, UnannotatedBinding, compute_inlay_hints};

/// In-memory binding store: the policy-side test double. Bindings are
/// appended in whatever order the caller likes; the policy must
/// normalize.
#[derive(Default)]
struct MockSource {
    bindings: Vec<UnannotatedBinding>,
}

impl MockSource {
    fn add(&mut self, after: usize, type_label: &str) {
        self.bindings.push(UnannotatedBinding {
            after,
            type_label: type_label.to_string(),
        });
    }
}

impl BindingTypeSource for MockSource {
    fn unannotated_bindings(&self) -> Vec<UnannotatedBinding> {
        self.bindings.clone()
    }
}

#[test]
fn bindings_pass_through_in_source_order() {
    let mut src = MockSource::default();
    src.add(18, "Point");
    src.add(64, "i32");
    let out = compute_inlay_hints(&src);
    let offsets: Vec<usize> = out.iter().map(|b| b.after).collect();
    assert_eq!(offsets, vec![18, 64], "recorded order is kept");
    assert_eq!(out[0].type_label, "Point");
    assert_eq!(out[1].type_label, "i32");
}

#[test]
fn empty_labels_are_dropped() {
    // An empty label cannot render as `: ` — a defensive skip, not a
    // wire-visible empty hint.
    let mut src = MockSource::default();
    src.add(10, "");
    src.add(20, "i32");
    let out = compute_inlay_hints(&src);
    let offsets: Vec<usize> = out.iter().map(|b| b.after).collect();
    assert_eq!(offsets, vec![20], "the empty label is dropped");
}

#[test]
fn rewinding_offsets_are_dropped_keeps_monotonic() {
    // The production source sorts, so a rewind would mean an internal
    // bug; dropping (not reordering) keeps the same failure class the
    // semantic-tokens policy chose: a missing hint, never a scrambled
    // wire order.
    let mut src = MockSource::default();
    src.add(20, "Point");
    src.add(5, "i32");
    src.add(30, "str");
    let out = compute_inlay_hints(&src);
    let offsets: Vec<usize> = out.iter().map(|b| b.after).collect();
    assert_eq!(offsets, vec![20, 30], "output stays strictly monotonic");
}

#[test]
fn duplicate_offsets_keep_the_first() {
    let mut src = MockSource::default();
    src.add(20, "Point");
    src.add(20, "Vec<Point>");
    let out = compute_inlay_hints(&src);
    assert_eq!(
        out,
        vec![UnannotatedBinding {
            after: 20,
            type_label: "Point".to_string(),
        }],
        "one hint per attachment offset"
    );
}

#[test]
fn empty_source_yields_no_hints() {
    let src = MockSource::default();
    assert!(compute_inlay_hints(&src).is_empty());
}
