//! Tests for `PlaceholderRegion` and `Region::Placeholder`

use crate::{BoundRegionKind, PlaceholderRegion, Region, UniverseIndex};
use glyim_core::interner::Interner;

#[test]
fn placeholder_region_fields() {
    let interner = Interner::new();
    let name = interner.intern("a");
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(2),
        bound: BoundRegionKind::BrNamed(name),
        index: 5,
    };

    assert_eq!(placeholder.universe.0, 2);
    assert_eq!(placeholder.index, 5);
    assert!(matches!(placeholder.bound, BoundRegionKind::BrNamed(_)));
}

#[test]
fn placeholder_region_anon() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(0),
        bound: BoundRegionKind::BrAnon(42),
        index: 0,
    };

    assert_eq!(placeholder.universe.0, 0);
    assert!(matches!(placeholder.bound, BoundRegionKind::BrAnon(42)));
}

#[test]
fn placeholder_region_env() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrEnv,
        index: 3,
    };

    assert!(matches!(placeholder.bound, BoundRegionKind::BrEnv));
}

#[test]
fn region_placeholder_variant() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };

    let region = Region::Placeholder(placeholder.clone());
    assert!(matches!(region, Region::Placeholder(_)));

    if let Region::Placeholder(p) = &region {
        assert_eq!(p.universe.0, 1);
        assert_eq!(p.index, 0);
    } else {
        panic!("Expected Placeholder variant");
    }
}

#[test]
fn region_placeholder_equality() {
    let p1 = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let p2 = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    assert_eq!(Region::Placeholder(p1), Region::Placeholder(p2));
}

#[test]
fn region_placeholder_inequality_different_universe() {
    let p1 = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let p2 = PlaceholderRegion {
        universe: UniverseIndex(2),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    assert_ne!(Region::Placeholder(p1), Region::Placeholder(p2));
}

#[test]
fn region_placeholder_inequality_different_index() {
    let p1 = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let p2 = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 1,
    };
    assert_ne!(Region::Placeholder(p1), Region::Placeholder(p2));
}

#[test]
fn region_placeholder_distinct_from_other_variants() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(0),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let region = Region::Placeholder(placeholder);

    assert_ne!(region, Region::Static);
    assert_ne!(region, Region::Erased);
    assert_ne!(region, Region::Error);
}

#[test]
fn universe_index_default_is_zero() {
    assert_eq!(UniverseIndex(0).0, 0);
}

#[test]
fn universe_index_equality() {
    assert_eq!(UniverseIndex(3), UniverseIndex(3));
    assert_ne!(UniverseIndex(3), UniverseIndex(4));
}

#[test]
fn placeholder_region_debug_format() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 3,
    };
    let debug_str = format!("{placeholder:?}");
    assert!(
        debug_str.contains("PlaceholderRegion"),
        "Debug output should contain PlaceholderRegion: {debug_str}"
    );
}

#[test]
fn region_placeholder_debug_format() {
    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 3,
    };
    let region = Region::Placeholder(placeholder);
    let debug_str = format!("{region:?}");
    assert!(
        debug_str.contains("Placeholder"),
        "Debug output should contain Placeholder: {debug_str}"
    );
}
