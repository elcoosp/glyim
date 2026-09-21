use crate::interner::Interner;
use crate::path::{Path, PathKind, PathSegment};

#[test]
fn from_single_as_name() {
    let interner = Interner::new();
    let name = interner.intern("foo");
    let path = Path::from_single(name);
    assert_eq!(path.segments.len(), 1);
    assert_eq!(path.segments[0].name, name);
    assert_eq!(path.kind, PathKind::Plain);
    assert_eq!(path.as_name(), Some(name));
}

#[test]
fn as_name_none() {
    let interner = Interner::new();
    let name = interner.intern("foo");
    let mut path = Path::from_single(name);
    path.kind = PathKind::SelfPath;
    assert_eq!(path.as_name(), None);
    path.kind = PathKind::Plain;
    path.segments.push(PathSegment {
        name,
        generic_args: None,
    });
    assert_eq!(path.as_name(), None);
}

// Plan §1.1: multi-segment paths with per-segment generic arguments must
// round-trip through `Path`. `foo::Bar<T, U>::baz` keeps `Bar`'s args `T, U`
// and `baz`'s (none) distinct.
#[test]
fn generic_args_round_trip() {
    let interner = Interner::new();
    let foo = interner.intern("foo");
    let bar = interner.intern("Bar");
    let t = interner.intern("T");
    let u = interner.intern("U");
    let baz = interner.intern("baz");

    let path = Path::from_segments(vec![
        PathSegment {
            name: foo,
            generic_args: None,
        },
        PathSegment {
            name: bar,
            generic_args: Some(vec![t, u]),
        },
        PathSegment {
            name: baz,
            generic_args: None,
        },
    ]);

    assert_eq!(path.segments.len(), 3);
    assert_eq!(path.segments[1].name, bar);
    assert_eq!(
        path.segments[1].generic_args,
        Some(vec![t, u]),
        "Bar's generic args must survive the round-trip"
    );
    assert_eq!(path.segments[2].generic_args, None);

    // Convenience constructor for a single generic segment.
    let five = interner.intern("Five");
    let single = Path::from_single_with_args(five, vec![t]);
    assert_eq!(single.segments[0].name, five);
    assert_eq!(single.segments[0].generic_args, Some(vec![t]));
}
