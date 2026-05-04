use glyim_hir::monomorphize::{discover_instantiations, apply_specializations, MonoResult};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;

fn lower_and_typeck(source: &str) -> (Hir, Interner, Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
    let parse_out = glyim_parse::parse(source);
    assert!(parse_out.errors.is_empty(), "parse errors: {:?}", parse_out.errors);
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

    let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
    let _ = typeck.check(&hir);
    let mut interner = typeck.interner.clone();
    let expr_types = typeck.expr_types.clone();
    let call_type_args = typeck.call_type_args.clone();

    (hir, interner, expr_types, call_type_args)
}

#[test]
fn two_pass_separation_no_rewrite_during_discovery() {
    let source = "fn id<T>(x: T) -> T { x }\nfn map_id<T>(v: T) -> T { id(v) }\nmain = () => map_id(42)";
    let (hir, mut interner, expr_types, call_type_args) = lower_and_typeck(source);

    let specs = discover_instantiations(&hir, &mut interner, &expr_types, &call_type_args);
    let mono = apply_specializations(&hir, &mut interner, &specs, &expr_types);

    // After specialization, no items should contain unresolved type parameters
    for item in &mono.hir.items {
        match item {
            glyim_hir::HirItem::Fn(f) => {
                let name = interner.resolve(f.name);
                assert!(
                    !name.contains("__T") && !name.contains("__K") && !name.contains("__V"),
                    "function {} still has unresolved type params in name", name
                );
            }
            glyim_hir::HirItem::Struct(s) => {
                let name = interner.resolve(s.name);
                assert!(
                    !name.contains("__T") && !name.contains("__K") && !name.contains("__V"),
                    "struct {} still has unresolved type params in name", name
                );
            }
            _ => {}
        }
    }

    // Verify specialized id__i64 exists
    let has_id_i64 = mono.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Fn(f) = item {
            interner.resolve(f.name) == "id__i64"
        } else {
            false
        }
    });
    assert!(has_id_i64, "expected specialized id__i64 function");

    // Verify specialized map_id__i64 exists
    let has_map_id_i64 = mono.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Fn(f) = item {
            interner.resolve(f.name) == "map_id__i64"
        } else {
            false
        }
    });
    assert!(has_map_id_i64, "expected specialized map_id__i64 function");
}

#[test]
fn discovery_detects_all_instantiations() {
    let source = "fn id<T>(x: T) -> T { x }\nmain = () => { let a = id(42); let b = id(true); a }";
    let (hir, mut interner, expr_types, call_type_args) = lower_and_typeck(source);

    let specs = discover_instantiations(&hir, &mut interner, &expr_types, &call_type_args);

    // Should have two instantiations of id: one for i64, one for bool
    assert!(specs.len() >= 2, "expected at least 2 specializations, got {}", specs.len());
}
