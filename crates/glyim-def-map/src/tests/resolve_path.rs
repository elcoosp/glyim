//! S10-T02: `Resolver::resolve_path` finds items in parent modules

use super::test_utils::{find_child_module, parse_and_build, resolver_for};
use glyim_core::path::{Path, PathKind, PathSegment};

/// Helper to build a simple single-segment path.
fn simple_path(name_str: &str, interner: &glyim_core::interner::Interner) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name_str),
            generic_args: None,
        }],
        kind: PathKind::Plain,
    }
}

/// Helper to build a `self::name` path.
fn self_path(name_str: &str, interner: &glyim_core::interner::Interner) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name_str),
            generic_args: None,
        }],
        kind: PathKind::SelfPath,
    }
}

/// Helper to build a `super::name` path.
fn super_path(name_str: &str, interner: &glyim_core::interner::Interner) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name_str),
            generic_args: None,
        }],
        kind: PathKind::Super(1),
    }
}

/// Helper to build a `crate::name` path.
fn crate_path(name_str: &str, interner: &glyim_core::interner::Interner) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name_str),
            generic_args: None,
        }],
        kind: PathKind::Crate,
    }
}

/// Helper to build a multi-segment path like `foo::bar`.
fn multi_path(
    segments: &[&str],
    kind: PathKind,
    interner: &glyim_core::interner::Interner,
) -> Path {
    Path {
        segments: segments
            .iter()
            .map(|s| PathSegment {
                name: interner.intern(s),
                generic_args: None,
            })
            .collect(),
        kind,
    }
}

#[test]
fn test_resolve_simple_name_in_same_module() {
    let source = "fn foo() {}";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = simple_path("foo", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(result.values.is_some(), "should resolve 'foo' as a value");
    assert!(
        result.types.is_none(),
        "foo should not be in types namespace"
    );
}

#[test]
fn test_resolve_struct_as_type() {
    let source = "struct Foo;";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = simple_path("Foo", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(result.types.is_some(), "should resolve 'Foo' as a type");
    assert!(
        result.values.is_none(),
        "Foo should not be in values namespace"
    );
}

#[test]
fn test_resolve_self_path() {
    let source = "fn foo() {}";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = self_path("foo", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(
        result.values.is_some(),
        "should resolve 'self::foo' as a value"
    );
}

#[test]
fn test_resolve_super_path_from_child() {
    let source = r"
        fn parent_fn() {}
        mod child {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let child =
        find_child_module(&def_map, def_map.root, "child").expect("should find child module");
    let resolver = resolver_for(&def_map, child);
    let path = super_path("parent_fn", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(
        result.values.is_some(),
        "should resolve 'super::parent_fn' from child module"
    );
}

#[test]
fn test_resolve_crate_path_from_nested() {
    let source = r"
        pub fn root_fn() {}
        mod a {
            mod b {
                fn dummy() {}
            }
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let a = find_child_module(&def_map, def_map.root, "a").expect("should find a");
    let b = find_child_module(&def_map, a, "b").expect("should find b");
    let resolver = resolver_for(&def_map, b);
    let path = crate_path("root_fn", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(
        result.values.is_some(),
        "should resolve 'crate::root_fn' from nested module"
    );
}

#[test]
fn test_resolve_nested_module_path() {
    let source = r"
        mod outer {
            struct Inner;
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = multi_path(&["outer", "Inner"], PathKind::Plain, &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(
        result.types.is_some(),
        "should resolve 'outer::Inner' as a type"
    );
}

#[test]
fn test_resolve_nonexistent_name() {
    let source = "fn foo() {}";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = simple_path("nonexistent", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(result.is_none(), "should not resolve nonexistent name");
}

#[test]
fn test_resolve_module_child() {
    let source = r"
        mod my_mod {
            fn inner() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = simple_path("my_mod", &def_map.interner);
    let result = resolver.resolve_path(&path);
    assert!(
        result.types.is_some(),
        "module should be resolvable as a type in parent scope"
    );
}

#[test]
fn test_resolve_double_super() {
    let source = r"
        fn top_fn() {}
        mod a {
            mod b {
                fn dummy() {}
            }
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let a = find_child_module(&def_map, def_map.root, "a").expect("should find a");
    let b = find_child_module(&def_map, a, "b").expect("should find b");
    let resolver = resolver_for(&def_map, b);
    let path = Path {
        segments: vec![PathSegment {
            name: def_map.interner.intern("top_fn"),
            generic_args: None,
        }],
        kind: PathKind::Super(2),
    };
    let result = resolver.resolve_path(&path);
    assert!(
        result.values.is_some(),
        "should resolve 'super::super::top_fn' from b"
    );
}

#[test]
fn test_resolve_super_from_root_is_noop() {
    let source = "fn foo() {}";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let path = Path {
        segments: vec![PathSegment {
            name: def_map.interner.intern("foo"),
            generic_args: None,
        }],
        kind: PathKind::Super(1),
    };
    let result = resolver.resolve_path(&path);
    assert!(
        result.values.is_some(),
        "super from root should stay at root and resolve foo"
    );
}

#[test]
fn test_resolve_path_returns_visibility() {
    let source = r"
        pub fn public_fn() {}
        fn private_fn() {}
    ";
    let (def_map, _diags) = parse_and_build(source);
    let resolver = resolver_for(&def_map, def_map.root);
    let pub_path = simple_path("public_fn", &def_map.interner);
    let priv_path = simple_path("private_fn", &def_map.interner);

    let pub_result = resolver.resolve_path(&pub_path);
    let priv_result = resolver.resolve_path(&priv_path);

    assert_eq!(
        pub_result.values.map(|(_, v)| v),
        Some(glyim_core::primitives::Visibility::Public),
        "public_fn should have Public visibility"
    );
    assert_eq!(
        priv_result.values.map(|(_, v)| v),
        Some(glyim_core::primitives::Visibility::Inherited),
        "private_fn should have Inherited visibility"
    );
}
