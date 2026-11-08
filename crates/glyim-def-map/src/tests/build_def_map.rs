//! S10-T01: `build_def_map` constructs correct module hierarchy

use super::test_utils::{find_child_module, parse_and_build};
use crate::ModuleOrigin;
use glyim_core::primitives::Visibility;

#[test]
fn test_empty_source() {
    let (def_map, diags) = parse_and_build("");
    assert!(
        diags.is_empty(),
        "expected no diagnostics for empty source, got: {diags:?}"
    );
    assert_eq!(
        def_map.modules.len(),
        1,
        "expected exactly one module (root)"
    );
    let root = &def_map.modules[def_map.root];
    assert!(root.children.is_empty(), "root should have no children");
    assert!(root.scope.types.is_empty(), "root should have no types");
    assert!(root.scope.values.is_empty(), "root should have no values");
    assert!(root.parent.is_none(), "root should have no parent");
    assert!(
        matches!(root.origin, ModuleOrigin::CrateRoot),
        "root should be CrateRoot"
    );
}

#[test]
fn test_single_fn() {
    let (def_map, diags) = parse_and_build("fn main() {}");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(
        root.scope.values.len(),
        1,
        "expected one value in root scope"
    );
    let (name, (_id, vis, _span)) = root.scope.values.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "main", "expected function named 'main'");
    assert_eq!(
        *vis,
        Visibility::Inherited,
        "fn without pub should be Inherited"
    );
}

#[test]
fn test_pub_fn_visibility() {
    let (def_map, diags) = parse_and_build("pub fn foo() {}");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(
        root.scope.values.len(),
        1,
        "expected one value in root scope"
    );
    let (_name, (_id, vis, _span)) = root.scope.values.get_index(0).unwrap();
    assert_eq!(
        *vis,
        Visibility::Public,
        "pub fn should have Public visibility"
    );
}

#[test]
fn test_struct_in_types_namespace() {
    let (def_map, diags) = parse_and_build("struct Foo;");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.types.len(), 1, "expected one type in root scope");
    assert!(
        root.scope.values.is_empty(),
        "struct should not be in values namespace"
    );
    let (name, (_id, vis, _span)) = root.scope.types.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "Foo");
    assert_eq!(
        *vis,
        Visibility::Inherited,
        "struct without pub should be Inherited"
    );
}

#[test]
fn test_pub_struct_visibility() {
    let (def_map, diags) = parse_and_build("pub struct Foo;");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    let (_name, (_id, vis, _span)) = root.scope.types.get_index(0).unwrap();
    assert_eq!(
        *vis,
        Visibility::Public,
        "pub struct should have Public visibility"
    );
}

#[test]
fn test_enum_in_types_namespace() {
    let (def_map, diags) = parse_and_build("enum Color { Red, Green, Blue }");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.types.len(), 1, "expected one type in root scope");
    let (name, _) = root.scope.types.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "Color");
}

#[test]
fn test_const_in_values_namespace() {
    let (def_map, diags) = parse_and_build("const MAX: i32 = 100;");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(
        root.scope.values.len(),
        1,
        "expected one value in root scope"
    );
    let (name, _) = root.scope.values.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "MAX");
}

#[test]
fn test_static_in_values_namespace() {
    let (def_map, diags) = parse_and_build("static COUNT: i32 = 0;");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(
        root.scope.values.len(),
        1,
        "expected one value in root scope"
    );
    let (name, _) = root.scope.values.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "COUNT");
}

#[test]
fn test_multiple_items() {
    let source = r"
        fn main() {}
        struct Foo;
        enum Bar { A, B }
        const X: i32 = 1;
    ";
    let (def_map, diags) = parse_and_build(source);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.values.len(), 2, "expected two values (main, X)");
    assert_eq!(root.scope.types.len(), 2, "expected two types (Foo, Bar)");
}

#[test]
fn test_inline_module() {
    let source = r"
        mod inner {
            fn helper() {}
        }
    ";
    let (def_map, diags) = parse_and_build(source);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.children.len(), 1, "root should have one child module");

    let inner =
        find_child_module(&def_map, def_map.root, "inner").expect("should find inner module");
    let inner_data = &def_map.modules[inner];
    assert_eq!(
        inner_data.parent,
        Some(def_map.root),
        "inner's parent should be root"
    );
    assert_eq!(
        inner_data.scope.values.len(),
        1,
        "inner should have one value"
    );
    assert!(
        matches!(inner_data.origin, ModuleOrigin::Inline { .. }),
        "inner should be Inline origin"
    );
}

#[test]
fn test_nested_inline_modules() {
    let source = r"
        mod outer {
            mod inner {
                fn deep() {}
            }
        }
    ";
    let (def_map, diags) = parse_and_build(source);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");

    let outer =
        find_child_module(&def_map, def_map.root, "outer").expect("should find outer module");
    let inner = find_child_module(&def_map, outer, "inner").expect("should find inner module");

    let inner_data = &def_map.modules[inner];
    assert_eq!(
        inner_data.parent,
        Some(outer),
        "inner's parent should be outer"
    );
    assert_eq!(
        inner_data.scope.values.len(),
        1,
        "inner should have one value"
    );

    let outer_data = &def_map.modules[outer];
    assert_eq!(
        outer_data.parent,
        Some(def_map.root),
        "outer's parent should be root"
    );
}

#[test]
fn test_module_parent_tracking() {
    let source = r"
        mod a {
            mod b {
                fn f() {}
            }
        }
    ";
    let (def_map, _diags) = parse_and_build(source);

    let a = find_child_module(&def_map, def_map.root, "a").expect("should find module a");
    let b = find_child_module(&def_map, a, "b").expect("should find module b");

    assert_eq!(def_map.modules[a].parent, Some(def_map.root));
    assert_eq!(def_map.modules[b].parent, Some(a));
    assert_eq!(def_map.modules[def_map.root].parent, None);
}

#[test]
fn test_module_origin_inline() {
    let source = "mod foo { fn bar() {} }";
    let (def_map, _diags) = parse_and_build(source);
    let foo = find_child_module(&def_map, def_map.root, "foo").expect("should find foo");
    assert!(matches!(
        def_map.modules[foo].origin,
        ModuleOrigin::Inline { .. }
    ));
}

#[test]
fn test_crate_root_origin() {
    let (def_map, _diags) = parse_and_build("fn main() {}");
    assert!(matches!(
        def_map.modules[def_map.root].origin,
        ModuleOrigin::CrateRoot
    ));
}

#[test]
fn test_duplicate_module_error() {
    let source = r"
        mod foo { fn a() {} }
        mod foo { fn b() {} }
    ";
    let (_def_map, diags) = parse_and_build(source);
    assert!(
        !diags.is_empty(),
        "expected diagnostics for duplicate module"
    );
    let has_dup = diags.iter().any(|d| d.message.contains("duplicate module"));
    assert!(has_dup, "expected 'duplicate module' error, got: {diags:?}");
}

#[test]
fn test_duplicate_item_same_namespace_error() {
    let source = r"
        fn foo() {}
        fn foo() {}
    ";
    let (_def_map, diags) = parse_and_build(source);
    assert!(!diags.is_empty(), "expected diagnostics for duplicate item");
    let has_dup = diags
        .iter()
        .any(|d| d.message.contains("duplicate definition"));
    assert!(
        has_dup,
        "expected 'duplicate definition' error, got: {diags:?}"
    );
}

#[test]
fn test_same_name_different_namespaces() {
    let source = r"
        struct Foo;
        fn Foo() {}
    ";
    let (def_map, diags) = parse_and_build(source);
    assert!(
        diags.is_empty(),
        "same name in different namespaces should be OK, got: {diags:?}"
    );
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.types.len(), 1, "expected one type");
    assert_eq!(root.scope.values.len(), 1, "expected one value");
}

#[test]
fn test_type_alias_in_types_namespace() {
    let (def_map, diags) = parse_and_build("type Point = (i32, i32);");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.types.len(), 1, "expected one type");
    let (name, _) = root.scope.types.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "Point");
}

#[test]
fn test_impl_in_types_namespace() {
    let (def_map, diags) = parse_and_build("impl Foo { fn bar() {} }");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(
        root.scope.types.len(),
        1,
        "impl should be in types namespace"
    );
}

#[test]
fn test_trait_def_in_types_namespace() {
    let (def_map, diags) = parse_and_build("trait Display { fn fmt(); }");
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.scope.types.len(), 1, "expected one type");
    let (name, _) = root.scope.types.get_index(0).unwrap();
    let name_str = def_map.interner.resolve(*name);
    assert_eq!(name_str, "Display");
}

#[test]
fn test_module_def_ids() {
    let source = "mod foo { fn bar() {} }";
    let (def_map, _diags) = parse_and_build(source);
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.def_id.to_raw(), 0, "root def_id should be 0");
    let foo = find_child_module(&def_map, def_map.root, "foo").expect("should find foo");
    let foo_data = &def_map.modules[foo];
    assert_ne!(
        foo_data.def_id.to_raw(),
        root.def_id.to_raw(),
        "child def_id should differ from root"
    );
}

#[test]
fn test_def_ids_are_unique() {
    let source = r"
        fn a() {}
        struct B;
        mod c { fn d() {} }
    ";
    let (def_map, _diags) = parse_and_build(source);
    let root = &def_map.modules[def_map.root];
    let mut seen_def_ids: Vec<u32> = vec![root.def_id.to_raw()];

    // Modules are now declared in the parent's type namespace, so their def_ids
    // appear both in root.scope.types and in the child ModuleData.def_id.
    // We collect all def_ids from scopes (which include module entries) and
    // then verify child module def_ids are consistent.
    for (_, (id, _, _)) in &root.scope.types {
        let raw = id.to_raw();
        assert!(!seen_def_ids.contains(&raw), "duplicate def_id: {raw}");
        seen_def_ids.push(raw);
    }
    for (_, (id, _, _)) in &root.scope.values {
        let raw = id.to_raw();
        assert!(!seen_def_ids.contains(&raw), "duplicate def_id: {raw}");
        seen_def_ids.push(raw);
    }

    let c = find_child_module(&def_map, def_map.root, "c").expect("should find c");
    let c_data = &def_map.modules[c];
    // The module's def_id should already be in seen_def_ids (from root.scope.types)
    assert!(
        seen_def_ids.contains(&c_data.def_id.to_raw()),
        "module c's def_id should appear in parent scope types"
    );

    for (_, (id, _, _)) in &c_data.scope.values {
        let raw = id.to_raw();
        assert!(!seen_def_ids.contains(&raw), "duplicate def_id: {raw}");
        seen_def_ids.push(raw);
    }
}

#[test]
fn test_module_with_items() {
    let source = r"
        mod network {
            struct Connection;
            fn connect() {}
            pub fn disconnect() {}
        }
    ";
    let (def_map, diags) = parse_and_build(source);
    assert!(diags.is_empty(), "expected no diagnostics, got: {diags:?}");
    let net =
        find_child_module(&def_map, def_map.root, "network").expect("should find network module");
    let net_data = &def_map.modules[net];
    assert_eq!(
        net_data.scope.types.len(),
        1,
        "network should have one type"
    );
    assert_eq!(
        net_data.scope.values.len(),
        2,
        "network should have two values"
    );

    let connect_name = def_map.interner.intern("connect");
    let connect_vis = net_data
        .scope
        .values
        .iter()
        .find(|(n, _)| **n == connect_name)
        .map(|(_, v)| v.1.clone());
    assert_eq!(
        connect_vis,
        Some(Visibility::Inherited),
        "connect should be Inherited"
    );

    let disconnect_name = def_map.interner.intern("disconnect");
    let disconnect_vis = net_data
        .scope
        .values
        .iter()
        .find(|(n, _)| **n == disconnect_name)
        .map(|(_, v)| v.1.clone());
    assert_eq!(
        disconnect_vis,
        Some(Visibility::Public),
        "disconnect should be Public"
    );
}
