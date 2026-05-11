use crate::item::HirItem;
use crate::node::{HirExpr, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

fn lower_source(source: &str) -> (crate::Hir, Interner) {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() { panic!("parse errors: {:?}", parse_out.errors); }
    let mut interner = parse_out.interner;
    (crate::lower(&parse_out.ast, &mut interner), interner)
}

fn find_call_id(expr: &HirExpr, callee: Symbol) -> Option<ExprId> {
    match expr {
        HirExpr::Call { id, callee, .. } if matches!(callee.as_ref(), HirExpr::Ident { name, .. } if *name == callee) => Some(*id),
        HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
            HirStmt::Expr(e) => find_call_id(e, callee),
            HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } => find_call_id(value, callee),
            _ => None,
        }),
        HirExpr::If { then_branch, else_branch, .. } => find_call_id(then_branch, callee)
            .or_else(|| else_branch.as_ref().and_then(|e| find_call_id(e, callee))),
        HirExpr::Match { arms, .. } => arms.iter().find_map(|arm| find_call_id(&arm.body, callee)),
        HirExpr::Return { value: Some(v), .. } => find_call_id(v, callee),
        _ => None,
    }
}

#[test]
fn mono_non_generic_passthrough() {
    let (hir, mut interner) = lower_source("main = () => 42");
    let result = super::monomorphize(&hir, &mut interner, &[], &HashMap::new());
    assert!(!result.hir.items.is_empty());
    assert!(!result.expr_types.is_empty());
}

#[test]
fn mono_generic_fn_with_call_type_args() {
    let (hir, mut interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
    let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
    let body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!() };
    let call_id = find_call_id(body, interner.intern("id")).expect("call id");
    let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
    let result = super::monomorphize(&hir, &mut interner, &[], &call_type_args);
    let has_spec = result.hir.items.iter().any(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name).starts_with("id__")));
    assert!(has_spec, "Should emit specialized id function");
}

#[test]
fn mangle_simple_type() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let mangled = super::mangle_type_name(&mut interner, vec_sym, &[HirType::Int]);
    assert_eq!(interner.resolve(mangled), "Vec__i64");
}

#[test]
fn mangle_multiple_type_args() {
    let mut interner = Interner::new();
    let hashmap_sym = interner.intern("HashMap");
    let mangled = super::mangle_type_name(&mut interner, hashmap_sym, &[HirType::Str, HirType::Int]);
    assert_eq!(interner.resolve(mangled), "HashMap__str_i64");
}

#[test]
fn mono_e2e_no_unresolved_type_params() {
    let src = "fn id<T>(x: T) -> T { x }\nmain = () => id(42)";
    let (hir, mut interner) = lower_source(src);
    let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
    let body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!() };
    let call_id = find_call_id(body, interner.intern("id")).expect("call id");
    let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
    let result = super::monomorphize(&hir, &mut interner, &[], &call_type_args);
    for item in &result.hir.items {
        if let HirItem::Fn(f) = item {
            crate::passes::no_type_params::assert_no_type_params(&f.body, &interner);
        }
    }
}

#[test]
fn mono_e2e_expr_types_fully_concrete() {
    let src = "fn id<T>(x: T) -> T { x }\nmain = () => id(42)";
    let (hir, mut interner) = lower_source(src);
    let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
    let body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!() };
    let call_id = find_call_id(body, interner.intern("id")).expect("call id");
    let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
    let result = super::monomorphize(&hir, &mut interner, &[], &call_type_args);
    let has_generic = result.expr_types.iter().any(|t| matches!(t, HirType::Generic(_, _)));
    assert!(!has_generic, "expr_types should be fully concrete");
}

#[test]
fn mono_split_works_via_monomorphize() {
    let src = "fn id<T>(x: T) -> T { x }\nfn map_id<T>(v: T) -> T { id(v) }\nmain = () => map_id(42)";
    let (hir, mut interner) = lower_source(src);
    let main_fn = hir.items.iter().find_map(|i| if let HirItem::Fn(f) = i && interner.resolve(f.name) == "main" { Some(f) } else { None }).unwrap();
    let map_id_call = find_call_id(&main_fn.body, interner.intern("map_id"));
    // For the test, we need to discover calls inside map_id. We'll run typeck to get real types.
    // But for this simple test we can manually provide call_type_args for the internal id(v) call.
    // Find the id call inside map_id's body.
    let map_id_fn = hir.items.iter().find_map(|i| {
        if let HirItem::Fn(f) = i && interner.resolve(f.name) == "map_id" { Some(f) } else { None }
    }).unwrap();
    let id_call_in_map = find_call_id(&map_id_fn.body, interner.intern("id")).expect("id call in map_id");
    let mut call_type_args = HashMap::new();
    if let Some(main_id) = map_id_call {
        call_type_args.insert(main_id, vec![HirType::Int]); // map_id(42) -> Int
    }
    call_type_args.insert(id_call_in_map, vec![HirType::Int]); // id(v) -> Int
    let expr_types = vec![HirType::Int; 100];
    let mono = super::monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    assert!(mono.hir.items.iter().any(|i| if let HirItem::Fn(f) = i { interner.resolve(f.name).starts_with("id__") } else { false }));
    assert!(mono.hir.items.iter().any(|i| if let HirItem::Fn(f) = i { interner.resolve(f.name).starts_with("map_id__") } else { false }));
}
