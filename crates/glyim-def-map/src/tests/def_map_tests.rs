use crate::*;
use glyim_core::def_id::CrateId;
use glyim_core::path::{Path, PathKind, PathSegment};
use glyim_core::primitives::Visibility;
use glyim_frontend::parse_to_syntax;
use glyim_span::FileId;

fn build_map(source: &str) -> (CrateDefMap, Vec<GlyimDiagnostic>) {
    let file_id = FileId::from_raw(0);
    let parse_result = parse_to_syntax(source, file_id);
    build_def_map(
        &parse_result.root,
        CrateId::from_raw(0),
        glyim_core::interner::Interner::new(),
    )
}

fn root_scope(def_map: &CrateDefMap) -> &ItemScope {
    &def_map.modules[def_map.root].scope
}

fn resolve_path(def_map: &CrateDefMap, path_str: &str) -> PerNs {
    let mut kind = PathKind::Plain;
    let remaining = if path_str.starts_with("crate::") {
        kind = PathKind::Crate;
        &path_str[7..]
    } else if path_str.starts_with("super::") {
        let depth = path_str.matches("super::").count() as u32;
        kind = PathKind::Super(depth);
        &path_str[(7 * depth as usize)..]
    } else if path_str.starts_with("self::") {
        kind = PathKind::SelfPath;
        &path_str[6..]
    } else {
        path_str
    };

    let segments: Vec<PathSegment> = remaining
        .split("::")
        .filter(|s| !s.is_empty())
        .map(|s| PathSegment {
            name: def_map.interner.intern(s),
            generic_args: None,
        })
        .collect();

    let path = Path { kind, segments };
    let resolver = Resolver::new(&def_map.modules, def_map.root, def_map.root);
    resolver.resolve_path(&path)
}

fn resolve_from(def_map: &CrateDefMap, module: ModuleId, path_str: &str) -> PerNs {
    let kind = if path_str.starts_with("super::") {
        let depth = path_str.matches("super::").count() as u32;
        PathKind::Super(depth)
    } else {
        PathKind::Plain
    };
    let remaining = match kind {
        PathKind::Super(d) => &path_str[(7 * d as usize)..],
        _ => path_str,
    };
    let segments: Vec<PathSegment> = remaining
        .split("::")
        .filter(|s| !s.is_empty())
        .map(|s| PathSegment {
            name: def_map.interner.intern(s),
            generic_args: None,
        })
        .collect();
    let path = Path { kind, segments };
    let resolver = Resolver::new(&def_map.modules, def_map.root, module);
    resolver.resolve_path(&path)
}

#[test]
fn t01_empty_file_creates_root_module() {
    let (def_map, diags) = build_map("");
    assert!(diags.is_empty());
    let root = def_map.root;
    let data = &def_map.modules[root];
    assert!(data.parent.is_none());
    assert!(data.children.is_empty());
    assert!(data.scope.types.is_empty());
    assert!(data.scope.values.is_empty());
    assert!(data.scope.macros.is_empty());
}

#[test]
fn t02_single_fn_appears_in_scope() {
    let (def_map, diags) = build_map("fn hello() {}");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.values.len(), 1);
    let (name, (_, vis, _)) = scope.values.iter().next().unwrap();
    assert_eq!(*name, def_map.interner.intern("hello"));
    assert_eq!(vis, &Visibility::Inherited);
    assert!(scope.types.is_empty());
}

#[test]
fn t03_struct_enum_trait_impl_in_scope() {
    let (def_map, diags) = build_map(
        "
        pub struct S;
        enum E {}
        trait T {}
        impl T for S {}
    ",
    );
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.types.len(), 4);
    assert!(scope.values.is_empty());
}

#[test]
fn t04_inline_module_child() {
    let (def_map, diags) = build_map(
        "
        mod foo {
            fn inner() {}
        }
    ",
    );
    assert!(diags.is_empty());
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.children.len(), 1);
    let child = root.children[0].1;
    let child_data = &def_map.modules[child];
    assert_eq!(child_data.scope.values.len(), 1);
    assert_eq!(
        child_data
            .scope
            .values
            .get_index(0)
            .unwrap()
            .0
            .clone()
            .clone(),
        def_map.interner.intern("inner")
    );
}

#[test]
fn t05_visibility_pub_vs_private() {
    let (def_map, diags) = build_map(
        "
        pub fn a() {}
        fn b() {}
        pub struct C;
        struct D;
    ",
    );
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.values.get_index(0).unwrap().1.1, Visibility::Public);
    assert_eq!(
        scope.values.get_index(1).unwrap().1.1,
        Visibility::Inherited
    );
    assert_eq!(scope.types.get_index(0).unwrap().1.1, Visibility::Public);
    assert_eq!(scope.types.get_index(1).unwrap().1.1, Visibility::Inherited);
}

#[test]
fn t06_plain_path_resolution() {
    let (def_map, diags) = build_map("fn bar() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "bar");
    assert!(result.types.is_none());
    assert!(result.values.is_some());
    let (id, vis) = result.values.unwrap();
    assert!(id.to_raw() > 0);
    assert_eq!(vis, Visibility::Inherited);
}

#[test]
fn t07_self_path_resolution() {
    let (def_map, diags) = build_map("fn baz() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "self::baz");
    assert!(result.values.is_some());
}

#[test]
fn t08_super_path_resolution() {
    // super:: from a top-level module refers to the crate root
    let (def_map, diags) = build_map(
        "
        fn crate_fn() {}
        mod my_mod {
            fn mod_fn() {}
        }
    ",
    );
    assert!(diags.is_empty());
    let mod_id = def_map.modules[def_map.root].children[0].1;

    // super::crate_fn should resolve from within my_mod
    let result = resolve_from(&def_map, mod_id, "super::crate_fn");
    assert!(
        result.values.is_some(),
        "expected crate_fn to be visible via super"
    );

    // super::mod_fn should NOT exist (mod_fn is in my_mod, not root)
    let result2 = resolve_from(&def_map, mod_id, "super::mod_fn");
    assert!(result2.is_none(), "mod_fn should not be visible via super");
}

#[test]
fn t09_crate_path_resolution() {
    let (def_map, diags) = build_map(
        "
        fn top() {}
        mod m {
            fn inner() {}
        }
    ",
    );
    assert!(diags.is_empty());
    let inner_id = def_map.modules[def_map.root].children[0].1;
    let path = Path {
        kind: PathKind::Crate,
        segments: vec![PathSegment {
            name: def_map.interner.intern("top"),
            generic_args: None,
        }],
    };
    let resolver = Resolver::new(&def_map.modules, def_map.root, inner_id);
    let result = resolver.resolve_path(&path);
    assert!(result.values.is_some());
}

#[test]
fn t10_duplicate_name_error() {
    let (_def_map, diags) = build_map(
        "
        fn dup() {}
        fn dup() {}
    ",
    );
    assert!(!diags.is_empty());
    let has_dup_error = diags
        .iter()
        .any(|d| d.message.contains("duplicate") || d.message.contains("already"));
    assert!(has_dup_error);
}

#[test]
fn t11_unknown_name_default() {
    let (def_map, diags) = build_map("fn existing() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "nonexistent");
    assert!(result.is_none());
}

// --- Additional tests beyond the original TDD plan ---

#[test]
fn t12_duplicate_across_namespaces_allowed() {
    // A type and a value can share the same name.
    let (def_map, diags) = build_map(
        "
        fn foo() {}
        struct foo;
    ",
    );
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.values.len(), 1);
    assert_eq!(scope.types.len(), 1);
    // Both have the same name
    assert_eq!(
        scope.values.get_index(0).unwrap().0.clone(),
        def_map.interner.intern("foo")
    );
    assert_eq!(
        scope.types.get_index(0).unwrap().0.clone(),
        def_map.interner.intern("foo")
    );
}

#[test]
fn t13_path_through_module() {
    let (def_map, diags) = build_map(
        "
        mod math {
            fn add() {}
        }
    ",
    );
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "math::add");
    assert!(result.values.is_some(), "math::add should resolve");
}

#[test]
fn t14_visibility_inherited_stored_correctly() {
    let (def_map, diags) = build_map("fn private_fn() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "private_fn");
    let (_id, vis) = result.values.expect("should resolve");
    assert_eq!(vis, Visibility::Inherited);
}

#[test]
fn t15_many_items() {
    let mut src = String::new();
    for i in 0..50 {
        src.push_str(&format!(
            "fn f{i}() {{}}
"
        ));
    }
    let (def_map, diags) = build_map(&src);
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.values.len(), 50);
}

#[test]
fn t16_self_path_through_module() {
    let (def_map, diags) = build_map(
        "
        mod m {
            fn inner() {}
        }
    ",
    );
    assert!(diags.is_empty());
    // self::m::inner
    let result = resolve_path(&def_map, "self::m::inner");
    assert!(result.values.is_some(), "self::m::inner should resolve");
}

#[test]
fn t17_crate_path_through_module() {
    let (def_map, diags) = build_map(
        "
        mod util {
            pub fn helper() {}
        }
    ",
    );
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "crate::util::helper");
    assert!(
        result.values.is_some(),
        "crate::util::helper should resolve"
    );
    let (_id, vis) = result.values.unwrap();
    assert_eq!(vis, Visibility::Public);
}

#[test]
fn t18_duplicate_inside_module_detected() {
    let (_def_map, diags) = build_map(
        "
        mod inner {
            fn dup() {}
            fn dup() {}
        }
    ",
    );
    assert!(!diags.is_empty());
    let has_dup = diags
        .iter()
        .any(|d| d.message.contains("duplicate") || d.message.contains("already"));
    assert!(has_dup);
}

#[test]
fn t19_resolve_nonexistent_module_path() {
    let (def_map, diags) = build_map("fn x() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "nonexistent::x");
    assert!(result.is_none());
}

#[test]
fn t24_use_decl_ignored() {
    let (def_map, diags) = build_map("use std::path; fn real() {}");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    // use decl is not collected
    assert_eq!(scope.values.len(), 1);
    assert_eq!(
        scope.values.get_index(0).unwrap().0.clone(),
        def_map.interner.intern("real")
    );
    assert!(scope.types.is_empty());
}

#[test]
fn t25_duplicate_name_different_modules_no_error() {
    let (_def_map, diags) = build_map("mod a { fn f() {} } mod b { fn f() {} }");
    assert!(diags.is_empty());
}

#[test]
fn t26_visibility_inside_module() {
    let (def_map, diags) = build_map("mod m { pub fn visible() {} fn hidden() {} }");
    assert!(diags.is_empty());
    let mod_id = def_map.modules[def_map.root].children[0].1;
    let mod_data = &def_map.modules[mod_id];
    assert_eq!(mod_data.scope.values.len(), 2);
    assert_eq!(
        mod_data.scope.values.get_index(0).unwrap().1.1,
        Visibility::Public
    );
    assert_eq!(
        mod_data.scope.values.get_index(1).unwrap().1.1,
        Visibility::Inherited
    );
}

#[test]
fn t27_unique_local_def_ids() {
    let (def_map, diags) = build_map("fn a() {} fn b() {} struct C; struct D;");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    let mut ids = Vec::new();
    for (_, (id, _, _)) in scope.values.iter().chain(scope.types.iter()) {
        ids.push(id.to_raw());
    }
    // All ids should be unique
    let unique: std::collections::HashSet<u32> = ids.iter().cloned().collect();
    assert_eq!(ids.len(), unique.len());
}

#[test]
fn t28_long_path_through_separate_modules() {
    let (def_map, diags) = build_map("mod a { fn af() {} } mod b { fn bf() {} }");
    assert!(diags.is_empty());
    let af = resolve_path(&def_map, "a::af");
    assert!(af.values.is_some());
    let bf = resolve_path(&def_map, "b::bf");
    assert!(bf.values.is_some());
}

#[test]
fn t29_empty_source_only_comments() {
    let (def_map, diags) = build_map("// just a comment");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert!(scope.types.is_empty());
    assert!(scope.values.is_empty());
    assert!(scope.macros.is_empty());
}

#[test]
fn t30_super_with_multi_segments() {
    let (def_map, diags) = build_map("fn top() {} mod inner { fn inside() {} }");
    assert!(diags.is_empty());
    let mod_id = def_map.modules[def_map.root].children[0].1;
    let result = resolve_from(&def_map, mod_id, "super::inner::inside");
    // Should resolve inside via super to root then down into inner
    assert!(result.values.is_some());
}

// --- Advanced path resolution tests ---

#[test]
fn t31_resolve_shadowed_name_returns_first() {
    // If two items share same name in same namespace, the first remains and second is error.
    // But we can test that the first item IS still found.
    let (def_map, diags) = build_map("fn shadow() {} fn shadow() {}");
    assert!(!diags.is_empty()); // duplicate error
    // The first definition should still be in scope
    let scope = root_scope(&def_map);
    assert!(
        scope
            .values
            .iter()
            .find_map(|(n, v)| if *n == def_map.interner.intern("shadow") {
                Some(v.clone())
            } else {
                None
            })
            .is_some()
    );
}

#[test]
fn t32_resolve_after_error_still_works() {
    // Even with duplicate errors, resolution for other names works.
    let (def_map, _diags) = build_map("fn a() {} fn a() {} fn b() {}");
    let result = resolve_path(&def_map, "b");
    assert!(result.values.is_some());
}

#[test]
fn t33_empty_module_body() {
    let (def_map, diags) = build_map("mod empty {}");
    assert!(diags.is_empty());
    let root = &def_map.modules[def_map.root];
    assert_eq!(root.children.len(), 1);
    let child = def_map.modules[root.children[0].1].clone();
    assert!(child.scope.types.is_empty());
    assert!(child.scope.values.is_empty());
}

#[test]
fn t34_multiple_modules_same_name_error() {
    let (_def_map, diags) = build_map("mod m {} mod m {}");
    assert!(!diags.is_empty());
}

#[test]
fn t35_path_resolution_case_sensitive() {
    let (def_map, diags) = build_map("fn Foo() {} fn foo() {}");
    assert!(diags.is_empty());
    let upper = resolve_path(&def_map, "Foo");
    let lower = resolve_path(&def_map, "foo");
    assert!(upper.values.is_some());
    assert!(lower.values.is_some());
    // They should have different IDs
    let id1 = upper.values.unwrap().0;
    let id2 = lower.values.unwrap().0;
    assert_ne!(id1, id2);
}

#[test]
fn t36_enum_variants_not_collected() {
    // Enum variants are NOT items in the def map; only the enum itself is.
    // This test verifies we don't accidentally collect variant names.
    let (def_map, diags) = build_map("enum Color { Red, Blue }");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.types.len(), 1); // only Color
    assert!(scope.values.is_empty());
}

#[test]
fn t37_trait_with_methods_only_trait_collected() {
    let (def_map, diags) = build_map("trait Hash { fn hash(&self) -> u64; }");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.types.len(), 1); // only Hash trait
    assert!(scope.values.is_empty());
}

#[test]
fn t38_struct_with_fields_only_struct_collected() {
    let (def_map, diags) = build_map("struct Point { x: f64, y: f64 }");
    assert!(diags.is_empty());
    let scope = root_scope(&def_map);
    assert_eq!(scope.types.len(), 1);
    assert!(scope.values.is_empty());
}

#[test]
fn t39_resolve_module_name_as_type() {
    // Modules are declared in the type namespace of their parent, so they
    // resolve as types. They should not appear in the value namespace.
    let (def_map, diags) = build_map("mod geometry {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "geometry");
    assert!(
        result.types.is_some(),
        "module name should resolve as a type"
    );
    assert!(
        result.values.is_none(),
        "module name should not resolve as a value"
    );
}

#[test]
fn t40_per_ns_from_types_constructor() {
    use glyim_core::def_id::LocalDefId;
    let id = LocalDefId::from_raw(42);
    let per_ns = PerNs::from_types(id, Visibility::Public);
    assert_eq!(per_ns.types, Some((id, Visibility::Public)));
    assert!(per_ns.values.is_none());
    assert!(per_ns.macros.is_none());
}

#[test]
fn t41_per_ns_is_none_when_empty() {
    let per_ns = PerNs::default();
    assert!(per_ns.is_none());
}

#[test]
fn t42_resolver_module_accessor() {
    let (def_map, diags) = build_map("fn x() {}");
    assert!(diags.is_empty());
    let resolver = Resolver::new(&def_map.modules, def_map.root, def_map.root);
    assert_eq!(resolver.module(), def_map.root);
    // def_map accessor returns reference
    let _: &glyim_core::arena::IndexVec<ModuleId, ModuleData> = resolver.def_map();
}

#[test]
fn t43_module_data_resolve_delegates_to_scope() {
    let (def_map, _) = build_map("fn test_fn() {}");
    let root_data = &def_map.modules[def_map.root];
    let result = root_data.resolve(def_map.interner.intern("test_fn"));
    assert!(result.is_some());
}

#[test]
fn t44_self_path_with_no_segments_returns_empty() {
    let (def_map, diags) = build_map("fn x() {}");
    assert!(diags.is_empty());
    // "self::" with no further segments
    let result = resolve_path(&def_map, "self::");
    assert!(result.is_none());
}

#[test]
fn t45_crate_path_with_no_segments_returns_empty() {
    let (def_map, diags) = build_map("fn x() {}");
    assert!(diags.is_empty());
    let result = resolve_path(&def_map, "crate::");
    assert!(result.is_none());
}
