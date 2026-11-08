//! S10-T03: Visibility checks prevent access to private items

use super::test_utils::{find_child_module, parse_and_build};
use crate::{CrateDefMap, ModuleId, is_accessible_from};
use glyim_core::primitives::Visibility;

/// Helper: check if a named item's visibility allows access from a given module.
fn item_is_accessible(
    def_map: &CrateDefMap,
    item_module: ModuleId,
    item_name: &str,
    from_module: ModuleId,
) -> bool {
    let name = def_map.interner.intern(item_name);

    let vis = def_map.modules[item_module]
        .scope
        .types
        .iter()
        .chain(def_map.modules[item_module].scope.values.iter())
        .find_map(|(n, v)| if *n == name { Some(v.1.clone()) } else { None });

    match vis {
        Some(v) => is_accessible_from(v, item_module, from_module, &def_map.modules),
        None => false,
    }
}

#[test]
fn test_public_item_accessible_from_child_module() {
    let source = r"
        pub fn public_fn() {}
        mod child {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let child =
        find_child_module(&def_map, def_map.root, "child").expect("should find child module");
    assert!(
        item_is_accessible(&def_map, def_map.root, "public_fn", child),
        "public item should be accessible from child module"
    );
}

#[test]
fn test_private_item_accessible_from_same_module() {
    let source = r"
        fn private_fn() {}
    ";
    let (def_map, _diags) = parse_and_build(source);
    assert!(
        item_is_accessible(&def_map, def_map.root, "private_fn", def_map.root),
        "private item should be accessible from its own module"
    );
}

#[test]
fn test_private_item_not_accessible_from_sibling_module() {
    let source = r"
        mod a {
            fn secret() {}
        }
        mod b {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let a = find_child_module(&def_map, def_map.root, "a").expect("should find a");
    let b = find_child_module(&def_map, def_map.root, "b").expect("should find b");
    assert!(
        !item_is_accessible(&def_map, a, "secret", b),
        "private item should not be accessible from sibling module"
    );
}

#[test]
fn test_inherited_item_accessible_from_descendant() {
    let source = r"
        fn parent_fn() {}
        mod child {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let child =
        find_child_module(&def_map, def_map.root, "child").expect("should find child module");
    assert!(
        item_is_accessible(&def_map, def_map.root, "parent_fn", child),
        "inherited-visibility item should be accessible from child module"
    );
}

#[test]
fn test_public_item_accessible_from_anywhere() {
    let source = r"
        pub struct Config;
        mod a {
            mod b {
                fn user() {}
            }
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let a = find_child_module(&def_map, def_map.root, "a").expect("should find a");
    let b = find_child_module(&def_map, a, "b").expect("should find b");
    assert!(
        item_is_accessible(&def_map, def_map.root, "Config", b),
        "public item should be accessible from deeply nested module"
    );
}

#[test]
fn test_is_accessible_from_public() {
    let source = "";
    let (def_map, _diags) = parse_and_build(source);
    assert!(
        is_accessible_from(
            Visibility::Public,
            def_map.root,
            def_map.root,
            &def_map.modules
        ),
        "Public items should always be accessible"
    );
}

#[test]
fn test_is_accessible_from_inherited_same_module() {
    let source = "";
    let (def_map, _diags) = parse_and_build(source);
    assert!(
        is_accessible_from(
            Visibility::Inherited,
            def_map.root,
            def_map.root,
            &def_map.modules
        ),
        "Inherited items should be accessible from the same module"
    );
}

#[test]
fn test_is_accessible_from_inherited_child_module() {
    let source = r"
        mod child {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let child = find_child_module(&def_map, def_map.root, "child").expect("should find child");
    assert!(
        is_accessible_from(Visibility::Inherited, def_map.root, child, &def_map.modules),
        "Inherited items should be accessible from descendant modules"
    );
}

#[test]
fn test_is_accessible_from_inherited_unrelated_module() {
    let source = r"
        mod a {
            fn dummy() {}
        }
        mod b {
            fn dummy() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let a = find_child_module(&def_map, def_map.root, "a").expect("should find a");
    let b = find_child_module(&def_map, def_map.root, "b").expect("should find b");
    assert!(
        !is_accessible_from(Visibility::Inherited, a, b, &def_map.modules),
        "Inherited items should NOT be accessible from unrelated modules"
    );
}

#[test]
fn test_module_visibility_tracked() {
    let source = r"
        pub mod public_mod {
            fn a() {}
        }
        mod private_mod {
            fn b() {}
        }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let pub_mod =
        find_child_module(&def_map, def_map.root, "public_mod").expect("should find public_mod");
    let priv_mod =
        find_child_module(&def_map, def_map.root, "private_mod").expect("should find private_mod");

    assert_eq!(
        def_map.modules[pub_mod].visibility,
        Visibility::Public,
        "pub mod should have Public visibility"
    );
    assert_eq!(
        def_map.modules[priv_mod].visibility,
        Visibility::Inherited,
        "non-pub mod should have Inherited visibility"
    );
}
