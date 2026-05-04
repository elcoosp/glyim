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

    fn walk(
        expr: &glyim_hir::HirExpr, interner: &Interner, expr_types: &mut Vec<HirType>,
        call_type_args: &mut HashMap<ExprId, Vec<HirType>>,
    ) {
        let id = expr.get_id();
        if id.as_usize() >= expr_types.len() { expr_types.resize(id.as_usize() + 1, HirType::Int); }
        match expr {
            glyim_hir::HirExpr::EnumVariant { id, enum_name, variant_name, args, .. } => {
                if interner.resolve(*enum_name) == "Option" && interner.resolve(*variant_name) == "Some" {
                    // Determine concrete inner type from the nested expression
                    fn extract_type(expr: &glyim_hir::HirExpr, interner: &Interner) -> HirType {
                        match expr {
                            glyim_hir::HirExpr::EnumVariant { id: _, enum_name, args, .. } => {
                                if interner.resolve(*enum_name) == "Option" && args.len() == 1 {
                                    HirType::Generic(*enum_name, vec![extract_type(&args[0], interner)])
                                } else {
                                    HirType::Named(*enum_name)
                                }
                            }
                            glyim_hir::HirExpr::IntLit { .. } => HirType::Int,
                            _ => HirType::Int,
                        }
                    }
                    if args.len() == 1 {
                        let inner_ty = extract_type(&args[0], interner);
                        call_type_args.insert(*id, vec![inner_ty]);
                    }
                }
                for a in args { walk(a, interner, expr_types, call_type_args); }
            }
            glyim_hir::HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        glyim_hir::HirStmt::Expr(e) => walk(e, interner, expr_types, call_type_args),
                        glyim_hir::HirStmt::Let { value, .. } | glyim_hir::HirStmt::LetPat { value, .. } => walk(value, interner, expr_types, call_type_args),
                        _ => {}
                    }
                }
            }
            glyim_hir::HirExpr::Match { scrutinee, arms, .. } => {
                walk(scrutinee, interner, expr_types, call_type_args);
                for (_, guard, body) in arms {
                    if let Some(g) = guard { walk(g, interner, expr_types, call_type_args); }
                    walk(body, interner, expr_types, call_type_args);
                }
            }
            glyim_hir::HirExpr::If { condition, then_branch, else_branch, .. } => {
                walk(condition, interner, expr_types, call_type_args);
                walk(then_branch, interner, expr_types, call_type_args);
                if let Some(e) = else_branch { walk(e, interner, expr_types, call_type_args); }
            }
            _ => {}
        }
    }

    for item in &hir.items {
        if let glyim_hir::HirItem::Fn(f) = item {
            walk(&f.body, interner, &mut expr_types, &mut call_type_args);
        }
    }
    (expr_types, call_type_args)
}

#[test]
fn enum_specialization_stored_in_context() {
    let source = r#"
enum Option<T> { Some(T), None }
main = () => {
    let x: Option<Option<i64>> = Option::Some(Option::Some(42));
    match x {
        Option::Some(inner) => match inner {
            Option::Some(val) => val,
            Option::None => 0,
        },
        Option::None => 0,
    }
}
"#;
    let (hir, mut interner) = simple_lower(source);
    let mut test_interner = interner.clone();
    let (expr_types, call_type_args) = build_call_type_args(&hir, &mut test_interner);

    let result = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    // Check that Option__i64 and Option__Option_i64 exist in the HIR
    let has_option_i64 = result.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Enum(e) = item {
            interner.resolve(e.name) == "Option__i64"
        } else { false }
    });
    let has_option_option_i64 = result.hir.items.iter().any(|item| {
        if let glyim_hir::HirItem::Enum(e) = item {
            interner.resolve(e.name) == "Option__Option_i64"
        } else { false }
    });
    assert!(has_option_i64, "Expected Option__i64 specialization");
    assert!(has_option_option_i64, "Expected Option__Option_i64 specialization");
}
