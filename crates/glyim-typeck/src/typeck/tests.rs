use super::{TypeChecker, TypeError};
use glyim_diag::Span;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::{HirExpr, HirStmt, HirBinOp, HirUnOp};
use glyim_interner::Interner;

fn typecheck(source: &str) -> TypeChecker {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        panic!("parse errors in test source: {:?}", parse_out.errors);
    }
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
    let mut tc = TypeChecker::new(interner);
    let _ = tc.check(&hir);
    tc
}

#[test]
fn lookup_unbound_returns_none() {
    let mut tc = TypeChecker::new(Interner::new());
    let missing = tc.interner.intern("nonexistent");
    assert_eq!(tc.lookup_binding(&missing), None);
}

#[test]
fn let_binding_visible_in_same_scope() {
    let mut tc = TypeChecker::new(Interner::new());
    let x = tc.interner.intern("x");
    tc.push_scope();
    tc.insert_binding(x, HirType::Int, false);
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Int));
}

#[test]
fn binding_not_visible_in_parent_scope() {
    let mut tc = TypeChecker::new(Interner::new());
    let _x = tc.interner.intern("x"); // unused, but harmless
    tc.push_scope();
    tc.push_scope();
    let y = tc.interner.intern("y");
    tc.insert_binding(y, HirType::Bool, false);
    tc.pop_scope();
    assert_eq!(tc.lookup_binding(&y), None);
}

#[test]
fn nested_scopes_shadow() {
    let mut tc = TypeChecker::new(Interner::new());
    let x = tc.interner.intern("x");
    tc.push_scope();
    tc.insert_binding(x, HirType::Int, false);
    tc.push_scope();
    tc.insert_binding(x, HirType::Bool, false);
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Bool));
    tc.pop_scope();
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Int));
}

#[test]
fn with_scope_restores_after_pop() {
    let mut tc = TypeChecker::new(Interner::new());
    let x = tc.interner.intern("x");
    tc.push_scope();
    tc.insert_binding(x, HirType::Int, false);
    let inner_result = tc.with_scope(|tc| {
        let y = tc.interner.intern("y");
        tc.insert_binding(y, HirType::Str, false);
        tc.lookup_binding(&y)
    });
    assert_eq!(inner_result, Some(HirType::Str));
    let y_later = tc.interner.intern("y"); // borrow mutable once
    assert_eq!(tc.lookup_binding(&y_later), None);
    assert_eq!(tc.lookup_binding(&x), Some(HirType::Int));
}

#[test]
fn register_struct_creates_entry() {
    let mut tc = typecheck("struct Point { x, y }\nmain = () => 0");
    let name = tc.interner.intern("Point");
    assert!(tc.structs.contains_key(&name));
    let info = &tc.structs[&name];
    assert_eq!(info.fields.len(), 2);
}

#[test]
fn register_struct_field_map_lookup() {
    let mut tc = typecheck("struct Point { x, y }\nmain = () => 0");
    let name = tc.interner.intern("Point");
    let info = &tc.structs[&name];
    let x = tc.interner.intern("x");
    let y = tc.interner.intern("y");
    assert!(info.field_map.contains_key(&x));
    assert!(info.field_map.contains_key(&y));
    assert_eq!(info.field_map[&x], 0);
    assert_eq!(info.field_map[&y], 1);
}

#[test]
fn register_enum_creates_entry() {
    let mut tc = typecheck("enum Color { Red, Green, Blue }\nmain = () => 0");
    let name = tc.interner.intern("Color");
    assert!(tc.enums.contains_key(&name));
    let info = &tc.enums[&name];
    assert_eq!(info.variants.len(), 3);
}

#[test]
fn register_enum_variant_map_lookup() {
    let mut tc = typecheck("enum Color { Red, Green }\nmain = () => 0");
    let name = tc.interner.intern("Color");
    let info = &tc.enums[&name];
    let red = tc.interner.intern("Red");
    let green = tc.interner.intern("Green");
    assert_eq!(info.variant_map[&red], 0);
    assert_eq!(info.variant_map[&green], 1);
}

#[test]
fn infer_int_lit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::IntLit {
        id: ExprId::new(0),
        value: 42,
        span: Span::new(0, 2),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn infer_float_lit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::FloatLit {
        id: ExprId::new(0),
        value: 3.14,
        span: Span::new(0, 4),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Float));
}

#[test]
fn infer_bool_lit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::BoolLit {
        id: ExprId::new(0),
        value: true,
        span: Span::new(0, 4),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Bool));
}

#[test]
fn infer_str_lit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::StrLit {
        id: ExprId::new(0),
        value: "hello".to_string(),
        span: Span::new(0, 7),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Str));
}

#[test]
fn infer_unit_lit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::UnitLit {
        id: ExprId::new(0),
        span: Span::new(0, 0),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Unit));
}

#[test]
fn infer_ident_unbound_falls_back_to_int() {
    let mut tc = TypeChecker::new(Interner::new());
    let name = tc.interner.intern("x");
    let expr = HirExpr::Ident {
        id: ExprId::new(0),
        name,
        span: Span::new(0, 1),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn infer_ident_returns_binding_type() {
    let mut tc = TypeChecker::new(Interner::new());
    let x = tc.interner.intern("x");
    tc.push_scope();
    tc.insert_binding(x, HirType::Bool, false);
    let expr = HirExpr::Ident {
        id: ExprId::new(0),
        name: x,
        span: Span::new(0, 1),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Bool));
}

#[test]
fn infer_binary_returns_int() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::Binary {
        id: ExprId::new(1),
        op: HirBinOp::Add,
        lhs: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 1, span: Span::new(0, 1) }),
        rhs: Box::new(HirExpr::IntLit { id: ExprId::new(2), value: 2, span: Span::new(4, 5) }),
        span: Span::new(0, 5),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn infer_unary_neg_returns_int() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::Unary {
        id: ExprId::new(1),
        op: HirUnOp::Neg,
        operand: Box::new(HirExpr::IntLit { id: ExprId::new(0), value: 5, span: Span::new(1, 2) }),
        span: Span::new(0, 2),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn infer_block_returns_last_expr_type() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::Block {
        id: ExprId::new(2),
        stmts: vec![
            HirStmt::Expr(HirExpr::IntLit { id: ExprId::new(0), value: 1, span: Span::new(0, 1) }),
            HirStmt::Expr(HirExpr::BoolLit { id: ExprId::new(1), value: true, span: Span::new(3, 7) }),
        ],
        span: Span::new(0, 8),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Bool));
}

#[test]
fn infer_empty_block_returns_unit() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::Block {
        id: ExprId::new(0),
        stmts: vec![],
        span: Span::new(0, 2),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Unit));
}

#[test]
fn infer_if_returns_then_type() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::If {
        id: ExprId::new(3),
        condition: Box::new(HirExpr::BoolLit { id: ExprId::new(0), value: true, span: Span::new(3, 7) }),
        then_branch: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 10, span: Span::new(10, 12) }),
        else_branch: None,
        span: Span::new(0, 12),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn infer_if_else_branches() {
    let mut tc = TypeChecker::new(Interner::new());
    let expr = HirExpr::If {
        id: ExprId::new(4),
        condition: Box::new(HirExpr::BoolLit { id: ExprId::new(0), value: true, span: Span::new(3, 7) }),
        then_branch: Box::new(HirExpr::IntLit { id: ExprId::new(1), value: 1, span: Span::new(10, 11) }),
        else_branch: Some(Box::new(HirExpr::BoolLit { id: ExprId::new(2), value: false, span: Span::new(17, 22) })),
        span: Span::new(0, 22),
    };
    assert_eq!(tc.check_expr(&expr), Some(HirType::Int));
}

#[test]
fn struct_lit_unknown_field_pushes_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => Point { x: 1, z: 3 }");
    let has_unknown_z = tc.errors.iter().any(|e| {
        if let TypeError::UnknownField { field, .. } = e {
            tc.interner.resolve(*field) == "z"
        } else { false }
    });
    assert!(has_unknown_z, "expected UnknownField error for 'z', got: {:?}", tc.errors);
}

#[test]
fn struct_lit_missing_field_pushes_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => Point { x: 1 }");
    let has_missing = tc.errors.iter().any(|e| {
        if let TypeError::MissingField { field, .. } = e {
            tc.interner.resolve(*field) == "y"
        } else { false }
    });
    assert!(has_missing, "expected MissingField error for 'y', got: {:?}", tc.errors);
}

#[test]
fn field_access_unknown_field_pushes_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.z }");
    let has_unknown = tc.errors.iter().any(|e| {
        if let TypeError::UnknownField { field, .. } = e {
            tc.interner.resolve(*field) == "z"
        } else { false }
    });
    assert!(has_unknown, "expected UnknownField error for 'z', got: {:?}", tc.errors);
}

#[test]
fn field_access_valid_field_no_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.x }");
    let field_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::UnknownField { .. })).collect();
    assert!(field_errors.is_empty(), "unexpected field errors: {:?}", field_errors);
}

#[test]
fn struct_lit_all_fields_present_no_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => Point { x: 1, y: 2 }");
    let struct_errors: Vec<_> = tc.errors.iter().filter(|e| {
        matches!(e, TypeError::UnknownField { .. } | TypeError::MissingField { .. } | TypeError::ExtraField { .. })
    }).collect();
    assert!(struct_errors.is_empty(), "unexpected struct errors: {:?}", struct_errors);
}

#[test]
fn match_exhaustive_with_wildcard_ok() {
    let tc = typecheck("enum Color { Red, Green }\nmain = () => match Color::Red { _ => 1 }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert!(exhaustive_errors.is_empty(), "unexpected non-exhaustive error: {:?}", exhaustive_errors);
}

#[test]
fn match_exhaustive_all_variants_ok() {
    let tc = typecheck("enum Color { Red, Green }\nmain = () => match Color::Red { Color::Red => 1, Color::Green => 2 }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert!(exhaustive_errors.is_empty(), "unexpected non-exhaustive error: {:?}", exhaustive_errors);
}

#[test]
fn match_non_exhaustive_pushes_error() {
    let tc = typecheck("enum Color { Red, Green, Blue }\nmain = () => match Color::Red { Color::Red => 1 }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert_eq!(exhaustive_errors.len(), 1, "expected exactly 1 NonExhaustiveMatch error, got: {:?}", tc.errors);
}

#[test]
fn match_on_non_enum_no_exhaustive_error() {
    let tc = typecheck("main = () => match 42 { 1 => 10, _ => 20 }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert!(exhaustive_errors.is_empty(), "matching on non-enum should not produce exhaustive error");
}

#[test]
fn match_option_some_none_exhaustive() {
    let tc = typecheck("main = () => { let m = Some(42); match m { Some(v) => v, None => 0 } }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert!(exhaustive_errors.is_empty(), "Some/None should be exhaustive");
}

#[test]
fn match_option_non_exhaustive_pushes_error() {
    let tc = typecheck("main = () => { let m = Some(42); match m { Some(v) => v } }");
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert_eq!(exhaustive_errors.len(), 1, "missing None variant should be non-exhaustive");
}

#[test]
fn cast_int_to_float_valid() {
    let tc = typecheck("main = () => 42 as f64");
    let cast_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::MismatchedTypes { .. })).collect();
    assert!(cast_errors.is_empty(), "int→float cast should be valid, got: {:?}", cast_errors);
}

#[test]
fn cast_int_to_str_invalid() {
    let tc = typecheck("main = () => 42 as Str");
    let cast_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::MismatchedTypes { .. })).collect();
    assert_eq!(cast_errors.len(), 1, "int→Str cast should be invalid");
}

#[test]
fn multiple_errors_accumulate() {
    let tc = typecheck("struct Point { x, y }\nmain = () => Point { x: 1, z: 3, w: 4 }");
    let field_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::UnknownField { .. })).collect();
    assert!(field_errors.len() >= 2, "expected ≥2 UnknownField errors, got {}: {:?}", field_errors.len(), tc.errors);
}

#[test]
fn check_returns_ok_when_no_errors() {
    let tc = typecheck("main = () => 42");
    assert!(tc.errors.is_empty(), "valid program should have no type errors");
}

#[test]
fn check_fn_return_mismatch_pushes_error() {
    let tc = typecheck("fn foo() -> bool { 42 }\nmain = () => foo()");
    let ret_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::InvalidReturnType { .. })).collect();
    assert_eq!(ret_errors.len(), 1, "return type mismatch should produce InvalidReturnType error");
}

#[test]
fn check_fn_params_bound_in_body() {
    let tc = typecheck("fn add(a, b) { a + b }\nmain = () => add(1, 2)");
    assert!(tc.errors.is_empty(), "param usage should typecheck: {:?}", tc.errors);
}
