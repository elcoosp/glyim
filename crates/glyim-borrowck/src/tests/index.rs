use crate::visitor::places_conflict;
use glyim_mir::{LocalIdx, Place, ProjectionElem};
use glyim_type::FieldIdx;

#[test]
fn different_index_locals_may_alias_and_conflict() {
    // MIR-21: two `Index` projections on the same base with DIFFERENT index
    // locals may still alias at runtime (`let i = 0; let j = i; arr[i]` vs
    // `arr[j]`), so they must conservatively conflict.
    let local = LocalIdx::from_raw(0);
    let idx0 = LocalIdx::from_raw(1);
    let idx1 = LocalIdx::from_raw(2);
    let place0 = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx0)]),
    };
    let place1 = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx1)]),
    };
    assert!(places_conflict(&place0, &place1));
    assert!(places_conflict(&place1, &place0));
}

#[test]
fn same_index_conflicts() {
    let local = LocalIdx::from_raw(0);
    let idx = LocalIdx::from_raw(1);
    let place_a = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx)]),
    };
    let place_b = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx)]),
    };
    assert!(places_conflict(&place_a, &place_b));
}

#[test]
fn whole_array_conflicts_with_element() {
    let local = LocalIdx::from_raw(0);
    let idx = LocalIdx::from_raw(1);
    let whole = Place::new(local);
    let element = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx)]),
    };
    // Whole array is a prefix of element -> conflict
    assert!(places_conflict(&whole, &element));
    assert!(places_conflict(&element, &whole));
}

#[test]
fn disjoint_fields_no_conflict_with_indices() {
    let local = LocalIdx::from_raw(0);
    let idx0 = LocalIdx::from_raw(1);
    let idx1 = LocalIdx::from_raw(2);
    let field0 = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };
    let elem0 = Place {
        local,
        projection: Box::new([ProjectionElem::Index(idx0)]),
    };
    // Field and Index are different projection kinds -> conservatively conflict
    assert!(!places_conflict(&field0, &elem0));
    // Two different index locals on the same base, even with matching
    // following projections, may alias at runtime (MIR-21) -> conflict.
    let elem0_field = Place {
        local,
        projection: Box::new([
            ProjectionElem::Index(idx0),
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };
    let elem1_field = Place {
        local,
        projection: Box::new([
            ProjectionElem::Index(idx1),
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };
    assert!(places_conflict(&elem0_field, &elem1_field));
}
