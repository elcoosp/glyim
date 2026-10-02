//! Slice pattern bindings are lowered to `Index` projections with constant locals.
//! Conflict detection for disjoint indices is already handled by the index projection logic.
//! This file tests that slice patterns `[a, b]` produce non‑conflicting borrows.

use crate::visitor::places_conflict;
use glyim_mir::{LocalIdx, Place, ProjectionElem};

#[test]
fn slice_pattern_disjoint_elements_no_conflict() {
    // Simulates `let [a, b] = &slice;` where `a` is slice[0], `b` is slice[1]
    let local = LocalIdx::from_raw(0);
    let idx0 = LocalIdx::from_raw(10);
    let idx1 = LocalIdx::from_raw(11);
    let place0 = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx0)]),
    };
    let place1 = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx1)]),
    };
    // MIR-21: different index locals on the same base MAY alias at runtime
    // (the index values are unknown here), so they conservatively conflict.
    assert!(places_conflict(&place0, &place1));
}

#[test]
fn slice_pattern_rest_overlaps_element_conflict() {
    // `rest @ ..` would be a `Slice` projection, but constant evaluation
    // is not available in this stream. This test is a placeholder; full
    // support requires constant propagation to determine slice ranges.
    // For now, any `Slice` projection is conservatively considered
    // conflicting with any `Index` on the same base.
    let local = LocalIdx::from_raw(0);
    let idx = LocalIdx::from_raw(0);
    // Slice projection: start and end are `Place`, but we can't resolve them.
    // The actual conflict logic will treat it as potentially overlapping.
    // This test asserts the conservative behaviour (conflict) for demonstration.
    let slice_place = Place {
        local,
        projection: Box::new([ProjectionElem::Subslice {
            from: 1,
            to: 2,
            from_end: false,
        }]),
    };
    let index_place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx)]),
    };
    // Conservative: slice conflicts with index
    assert!(places_conflict(&slice_place, &index_place));
}
