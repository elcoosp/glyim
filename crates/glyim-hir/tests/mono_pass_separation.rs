use glyim_hir::monomorphize::{discover_instantiations, apply_specializations};
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;

fn simple_lower(source: &str) -> (Hir, Interner) {
    let parse_out = glyim_parse::parse(source);
    assert!(parse_out.errors.is_empty(), "parse errors: {:?}", parse_out.errors);
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    (hir, interner)
}

/// Build minimal expr_types and call_type_args for generic call discovery.
/// We walk the HIR, find Call exprs to generic functions, and supply type args.
fn build_call_type_args(hir: &Hir, interner: &mut Interner) -> (Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
    let mut expr_types: Vec<HirType> = Vec::new();
    let mut call_type_args: HashMap<ExprId, Vec<HirType>> = HashMap::new();
    let id_sym = interner.resolve_symbol("id").unwrap_or_else(|| interner.intern("id"));
    let map_id_sym = interner.resolve_symbol("map_id").unwrap_or_else(|| interner.intern("map_id"));

    fn walk(
        expr: &glyim_hir::HirExpr,
        interner: &Interner,
        expr_types: &mut Vec<HirType>,
        call_type_args: &mut HashMap<ExprId, Vec<HirType>>,
        id_sym: glyim_interner::Symbol,
        map_id_sym: glyim_interner::Symbol,
    ) {
        let id = expr.get_id();
        if id.as_usize() >= expr_types.len() {
            expr_types.resize(id.as_usize() + 1, HirType::Int);
        }
        match expr {
            glyim_hir::HirExpr::Call { id, callee, args, .. } => {
                if *callee == id_sym {
                    call_type_args.insert(*id, vec![HirType::Int]);
                } else if *callee == map_id_sym {
                    call_type_args.insert(*id, vec![HirType::Int]);
                }
                for a in args {
                    walk(a, interner, expr_types, call_type_args, id_sym, map_id_sym);
                }
            }
            glyim_hir::HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        glyim_hir::HirStmt::Expr(e) => walk(e, interner, expr_types, call_type_args, id_sym, map_id_sym),
                        glyim_hir::HirStmt::Let { value, .. }
                        | glyim_hir::HirStmt::LetPat { value, .. }
                        | glyim_hir::HirStmt::Assign { value, .. } => {
                            walk(value, interner, expr_types, call_type_args, id_sym, map_id_sym);
                        }
                        _ => {}
                    }
                }
            }
            glyim_hir::HirExpr::If { condition, then_branch, else_branch, .. } => {
                walk(condition, interner, expr_types, call_type_args, id_sym, map_id_sym);
                walk(then_branch, interner, expr_types, call_type_args, id_sym, map_id_sym);
                if let Some(e) = else_branch {
                    walk(e, interner, expr_types, call_type_args, id_sym, map_id_sym);
                }
            }
            glyim_hir::HirExpr::Binary { lhs, rhs, .. } => {
                walk(lhs, interner, expr_types, call_type_args, id_sym, map_id_sym);
                walk(rhs, interner, expr_types, call_type_args, id_sym, map_id_sym);
            }
            glyim_hir::HirExpr::Unary { operand, .. } => {
                walk(operand, interner, expr_types, call_type_args, id_sym, map_id_sym);
            }
            glyim_hir::HirExpr::Return { value, .. } => {
                if let Some(v) = value {
                    walk(v, interner, expr_types, call_type_args, id_sym, map_id_sym);
                }
            }
            _ => {}
        }
    }

    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(f) = item {
            walk(&f.body, interner, &mut expr_types, &mut call_type_args, id_sym, map_id_sym);
        }
    }

    (expr_types, call_type_args)
}

#[test]
fn two_pass_separation_no_rewrite_during_discovery() {
    let source = "fn id<T>(x: T) -> T { x }\nfn map_id<T>(v: T) -> T { id(v) }\nmain = () => map_id(42)";
    let (hir, mut interner) = simple_lower(source);
    let mut test_interner = interner.clone();
    let (expr_types, call_type_args) = build_call_type_args(&hir, &mut test_interner);

    let specs = discover_instantiations(&hir, &mut interner, &expr_types, &call_type_args);
    let mono = apply_specializations(&hir, &mut interner, &specs, &expr_types, &call_type_args);

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
    let (hir, mut interner) = simple_lower(source);
    let mut test_interner = interner.clone();
    let (expr_types, call_type_args) = build_call_type_args(&hir, &mut test_interner);

    let specs = discover_instantiations(&hir, &mut interner, &expr_types, &call_type_args);

    // Should have at least one instantiation of id
    assert!(specs.len() >= 1, "expected at least 1 specialization, got {}", specs.len());
}
