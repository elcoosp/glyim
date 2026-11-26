use crate::*;
use std::collections::HashSet;

#[test]
fn place_equality_same_local_empty_projection() {
    let p1 = Place::new(LocalIdx::from_usize(0));
    let p2 = Place::new(LocalIdx::from_usize(0));
    assert_eq!(p1, p2);
}

#[test]
fn place_inequality_different_local() {
    let p1 = Place::new(LocalIdx::from_usize(0));
    let p2 = Place::new(LocalIdx::from_usize(1));
    assert_ne!(p1, p2);
}

#[test]
fn place_equality_with_same_projection() {
    let p1 = Place {
        local: LocalIdx::from_usize(0),
        projection: Box::new([ProjectionElem::Deref]),
    };
    let p2 = Place {
        local: LocalIdx::from_usize(0),
        projection: Box::new([ProjectionElem::Deref]),
    };
    assert_eq!(p1, p2);
}

#[test]
fn place_inequality_different_projection() {
    let p1 = Place {
        local: LocalIdx::from_usize(0),
        projection: Box::new([ProjectionElem::Deref]),
    };
    let p2 = Place {
        local: LocalIdx::from_usize(0),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };
    assert_ne!(p1, p2);
}

#[test]
fn place_hash_consistency() {
    let p1 = Place::new(LocalIdx::from_usize(0));
    let p2 = Place::new(LocalIdx::from_usize(0));
    let mut set = HashSet::new();
    set.insert(p1.clone());
    assert!(set.contains(&p2));
}

#[test]
fn projection_elem_equality() {
    assert_eq!(ProjectionElem::Deref, ProjectionElem::Deref);
    assert_eq!(
        ProjectionElem::Field(FieldIdx::from_usize(3)),
        ProjectionElem::Field(FieldIdx::from_usize(3))
    );
    assert_ne!(
        ProjectionElem::Field(FieldIdx::from_usize(0)),
        ProjectionElem::Field(FieldIdx::from_usize(1))
    );
    assert_eq!(
        ProjectionElem::Index(LocalIdx::from_usize(5)),
        ProjectionElem::Index(LocalIdx::from_usize(5))
    );
    assert_eq!(
        ProjectionElem::Downcast(VariantIdx::from_usize(0)),
        ProjectionElem::Downcast(VariantIdx::from_usize(0))
    );
}
