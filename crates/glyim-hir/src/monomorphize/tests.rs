use super::*;
use crate::node::{HirExpr, HirStmt};
use crate::item::HirItem;
use glyim_interner::Interner;

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
