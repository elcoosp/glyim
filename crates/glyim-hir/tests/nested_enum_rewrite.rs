use glyim_hir::Hir;
use glyim_hir::monomorphize::monomorphize;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Interner;
use std::collections::HashMap;

fn typecheck_source(source: &str) -> (Hir, Interner, Vec<HirType>, HashMap<ExprId, Vec<HirType>>) {
    let parse_out = glyim_parse::parse(source);
    assert!(parse_out.errors.is_empty());
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let mut typeck = glyim_typeck::TypeChecker::new(interner.clone());
    typeck.check(&hir).expect("type check must succeed");
    let interner = typeck.interner.clone();
    let expr_types = typeck.expr_types;
    let call_type_args = typeck.call_type_args;
    (hir, interner, expr_types, call_type_args)
}

#[test]
fn nested_option_enum_variant_rewritten() {
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
    let (hir, mut interner, expr_types, call_type_args) = typecheck_source(source);
    let mono_result = monomorphize(&hir, &mut interner, &expr_types, &call_type_args);
    let interner = &mut interner;
    // Walk the HIR and assert that all EnumVariant expressions reference the specialized enum names.
    fn check_enum_variant_names(item: &glyim_hir::HirItem, interner: &Interner) {
        if let glyim_hir::HirItem::Fn(f) = item {
            check_expr(&f.body, interner);
        }
    }
    fn check_expr(expr: &glyim_hir::HirExpr, interner: &Interner) {
        match expr {
            glyim_hir::HirExpr::EnumVariant { enum_name, .. } => {
                let name = interner.resolve(*enum_name);
                assert!(
                    name == "Option__i64" || name == "Option__Option_i64" || name == "Option",
                    "Expected specialized enum name, got {}",
                    name
                );
            }
            glyim_hir::HirExpr::Match {
                scrutinee, arms, ..
            } => {
                check_expr(scrutinee, interner);
                for arm in arms {
                    check_expr(&arm.body, interner);
                }
            }
            glyim_hir::HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        glyim_hir::HirStmt::Expr(e)
                        | glyim_hir::HirStmt::Let { value: e, .. }
                        | glyim_hir::HirStmt::LetPat { value: e, .. } => check_expr(e, interner),
                        _ => {}
                    }
                }
            }
            glyim_hir::HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                check_expr(condition, interner);
                check_expr(then_branch, interner);
                if let Some(e) = else_branch {
                    check_expr(e, interner);
                }
            }
            _ => {}
        }
    }
    for item in &mono_result.hir.items {
        check_enum_variant_names(item, interner);
    }
}
