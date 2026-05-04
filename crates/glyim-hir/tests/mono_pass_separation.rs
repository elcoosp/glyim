use glyim_hir::monomorphize::monomorphize;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::Hir;
use glyim_interner::Interner;
use std::collections::HashMap;

fn simple_lower(source: &str) -> (Hir, Interner) {
    let parse_out = glyim_parse::parse(source);
    assert!(parse_out.errors.is_empty());
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    (hir, interner)
}

fn build_call_type_args(hir: &Hir, interner: &mut Interner) -> (Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
    let mut expr_types: Vec<HirType> = Vec::new();
    let mut call_type_args: HashMap<ExprId, Vec<HirType>> = HashMap::new();
    let id_sym = interner.resolve_symbol("id").unwrap_or_else(|| interner.intern("id"));
    let map_id_sym = interner.resolve_symbol("map_id").unwrap_or_else(|| interner.intern("map_id"));

    fn walk(
        expr: &glyim_hir::HirExpr, interner: &Interner, expr_types: &mut Vec<HirType>,
        call_type_args: &mut HashMap<ExprId, Vec<HirType>>,
        id_sym: glyim_interner::Symbol, map_id_sym: glyim_interner::Symbol,
    ) {
        let id = expr.get_id();
        if id.as_usize() >= expr_types.len() { expr_types.resize(id.as_usize() + 1, HirType::Int); }
        match expr {
            glyim_hir::HirExpr::Call { id, callee, args, .. } => {
                if *callee == id_sym { call_type_args.insert(*id, vec![HirType::Int]); }
                else if *callee == map_id_sym { call_type_args.insert(*id, vec![HirType::Int]); }
                for a in args { walk(a, interner, expr_types, call_type_args, id_sym, map_id_sym); }
            }
            glyim_hir::HirExpr::Block { stmts, .. } => for stmt in stmts {
                match stmt {
                    glyim_hir::HirStmt::Expr(e) => walk(e, interner, expr_types, call_type_args, id_sym, map_id_sym),
                    glyim_hir::HirStmt::Let { value, .. } | glyim_hir::HirStmt::LetPat { value, .. } | glyim_hir::HirStmt::Assign { value, .. } => walk(value, interner, expr_types, call_type_args, id_sym, map_id_sym),
                    _ => {}
                }
            },
            glyim_hir::HirExpr::If { condition, then_branch, else_branch, .. } => {
                walk(condition, interner, expr_types, call_type_args, id_sym, map_id_sym);
                walk(then_branch, interner, expr_types, call_type_args, id_sym, map_id_sym);
                if let Some(e) = else_branch { walk(e, interner, expr_types, call_type_args, id_sym, map_id_sym); }
            }
            glyim_hir::HirExpr::Binary { lhs, rhs, .. } => {
                walk(lhs, interner, expr_types, call_type_args, id_sym, map_id_sym);
                walk(rhs, interner, expr_types, call_type_args, id_sym, map_id_sym);
            }
            glyim_hir::HirExpr::Unary { operand, .. } | glyim_hir::HirExpr::Return { value: Some(operand), .. } => walk(operand, interner, expr_types, call_type_args, id_sym, map_id_sym),
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
fn split_works_via_monomorphize() {
    let source = "fn id<T>(x: T) -> T { x }\nfn map_id<T>(v: T) -> T { id(v) }\nmain = () => map_id(42)";
    let (hir, mut interner) = simple_lower(source);
    let mut test_interner = interner.clone();
    let (expr_types, call_type_args) = build_call_type_args(&hir, &mut test_interner);

    let mono = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    assert!(mono.hir.items.iter().any(|i| if let glyim_hir::HirItem::Fn(f) = i { interner.resolve(f.name) == "id__i64" } else { false }));
    assert!(mono.hir.items.iter().any(|i| if let glyim_hir::HirItem::Fn(f) = i { interner.resolve(f.name) == "map_id__i64" } else { false }));
}
