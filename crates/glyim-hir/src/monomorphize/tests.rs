use super::*;
use crate::node::{HirExpr, HirStmt};
use crate::item::HirItem;
use crate::types::HirType;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

fn lower_source(source: &str) -> (crate::Hir, Interner) {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() { panic!("parse errors: {:?}", parse_out.errors); }
    let mut interner = parse_out.interner;
    (crate::lower(&parse_out.ast, &mut interner), interner)
}

#[test]
fn mono_non_generic_passthrough() {
    let (hir, mut interner) = lower_source("main = () => 42");
    let result = monomorphize(&hir, &mut interner, &[], &HashMap::new());
    assert_eq!(result.hir.items.len(), hir.items.len());
}

#[test]
fn mono_generic_fn_with_call_type_args() {
    let (hir, mut interner) = lower_source("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
    let main_fn = hir.items.iter().find(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name) == "main")).unwrap();
    let main_fn_body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!("expected Fn") };
    let call_id = find_call_id(main_fn_body, interner.intern("id")).expect("call id");
    let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
    let result = monomorphize(&hir, &mut interner, &[], &call_type_args);
    let has_specialized = result.hir.items.iter().any(|i| matches!(i, HirItem::Fn(f) if interner.resolve(f.name).starts_with("id__")));
    assert!(has_specialized);
}

fn find_call_id(expr: &HirExpr, callee: Symbol) -> Option<ExprId> {
    match expr {
        HirExpr::Call { id, callee: c, .. } if *c == callee => Some(*id),
        HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
            HirStmt::Expr(e) => find_call_id(e, callee),
            HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } => find_call_id(value, callee),
            _ => None,
        }),
        HirExpr::If { then_branch, else_branch, .. } => find_call_id(then_branch, callee).or_else(|| else_branch.as_ref().and_then(|e| find_call_id(e, callee))),
        HirExpr::Match { arms, .. } => arms.iter().find_map(|(_, _, body)| find_call_id(body, callee)),
        HirExpr::Return { value: Some(v), .. } => find_call_id(v, callee),
        _ => None,
    }
}

// ── New property tests ──────────────────────────────────────────────

#[test]
fn mangle_simple_type() {
    let mut interner = Interner::new();
    let base = interner.intern("Vec");
    let args = vec![HirType::Int];
    let mangled = mangle_type_name(&mut interner, base, &args);
    assert_eq!(interner.resolve(mangled), "Vec__i64");
}

#[test]
fn mangle_multiple_type_args() {
    let mut interner = Interner::new();
    let base = interner.intern("HashMap");
    let args = vec![HirType::Str, HirType::Int];
    let mangled = mangle_type_name(&mut interner, base, &args);
    assert_eq!(interner.resolve(mangled), "HashMap__str_i64");
}

#[test]
fn substitute_generic_with_nested_types() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    let u = interner.intern("U");
    let ty = HirType::Generic(
        interner.intern("Result"),
        vec![HirType::Named(t), HirType::Named(u)]
    );
    let mut sub = HashMap::new();
    sub.insert(t, HirType::Int);
    sub.insert(u, HirType::Str);
    let result = crate::types::substitute_type(&ty, &sub);
    assert_eq!(result, HirType::Generic(
        interner.intern("Result"),
        vec![HirType::Int, HirType::Str]
    ));
}

#[test]
fn monomorphize_eliminates_all_generics_for_known_types() {
    let src = "fn id<T>(x: T) -> T { x }\nfn main() -> i64 { id(42) }";
    let (hir, mut interner) = lower_source(src);
    let is_main = |i: &&HirItem| {
        if let HirItem::Fn(f) = i {
            interner.resolve(f.name) == "main"
        } else {
            false
        }
    };
    let main_fn = hir.items.iter().find(is_main).unwrap();
    let main_fn_body = if let HirItem::Fn(f) = main_fn { &f.body } else { panic!() };
    let call_id = find_call_id(main_fn_body, interner.intern("id")).expect("call id");
    let call_type_args = HashMap::from([(call_id, vec![HirType::Int])]);
    let result = monomorphize(&hir, &mut interner, &[], &call_type_args);
    let has_generic = result.hir.items.iter().any(|item| {
        if let HirItem::Fn(f) = item {
            f.params.iter().any(|(_, ty)| matches!(ty, HirType::Generic(_, _)))
        } else {
            false
        }
    });
    assert!(!has_generic, "monomorphize should eliminate all Generic types in concrete functions");
}
