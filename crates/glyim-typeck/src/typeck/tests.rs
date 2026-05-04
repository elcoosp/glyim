use glyim_hir::node::HirFn;
use glyim_hir::types::HirPattern;
use super::{TypeChecker, TypeError};
use glyim_diag::Span;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::{HirBinOp, HirExpr, HirStmt, HirUnOp};
use glyim_interner::Interner;

// Import internals via crate paths
use crate::unify;
use crate::typeck::resolver;

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
    let _x = tc.interner.intern("x");
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
    let y_later = tc.interner.intern("y");
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
        lhs: Box::new(HirExpr::IntLit {
            id: ExprId::new(0),
            value: 1,
            span: Span::new(0, 1),
        }),
        rhs: Box::new(HirExpr::IntLit {
            id: ExprId::new(2),
            value: 2,
            span: Span::new(4, 5),
        }),
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
        operand: Box::new(HirExpr::IntLit {
            id: ExprId::new(0),
            value: 5,
            span: Span::new(1, 2),
        }),
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
            HirStmt::Expr(HirExpr::IntLit {
                id: ExprId::new(0),
                value: 1,
                span: Span::new(0, 1),
            }),
            HirStmt::Expr(HirExpr::BoolLit {
                id: ExprId::new(1),
                value: true,
                span: Span::new(3, 7),
            }),
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
        condition: Box::new(HirExpr::BoolLit {
            id: ExprId::new(0),
            value: true,
            span: Span::new(3, 7),
        }),
        then_branch: Box::new(HirExpr::IntLit {
            id: ExprId::new(1),
            value: 10,
            span: Span::new(10, 12),
        }),
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
        condition: Box::new(HirExpr::BoolLit {
            id: ExprId::new(0),
            value: true,
            span: Span::new(3, 7),
        }),
        then_branch: Box::new(HirExpr::IntLit {
            id: ExprId::new(1),
            value: 1,
            span: Span::new(10, 11),
        }),
        else_branch: Some(Box::new(HirExpr::BoolLit {
            id: ExprId::new(2),
            value: false,
            span: Span::new(17, 22),
        })),
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
        } else {
            false
        }
    });
    assert!(has_unknown_z, "expected UnknownField error for 'z', got: {:?}", tc.errors);
}

#[test]
fn struct_lit_missing_field_pushes_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => Point { x: 1 }");
    let has_missing = tc.errors.iter().any(|e| {
        if let TypeError::MissingField { field, .. } = e {
            tc.interner.resolve(*field) == "y"
        } else {
            false
        }
    });
    assert!(has_missing, "expected MissingField error for 'y', got: {:?}", tc.errors);
}

#[test]
fn field_access_unknown_field_pushes_error() {
    let tc = typecheck("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.z }");
    let has_unknown = tc.errors.iter().any(|e| {
        if let TypeError::UnknownField { field, .. } = e {
            tc.interner.resolve(*field) == "z"
        } else {
            false
        }
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
    let src = "enum Option<T> { Some(T), None }\nmain = () => { let m = Option::Some(42); match m { Option::Some(v) => v, Option::None => 0 } }";
    let mut tc = typecheck(src);
    let exhaustive_errors: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::NonExhaustiveMatch { .. })).collect();
    assert!(exhaustive_errors.is_empty(), "Some/None should be exhaustive");
}

#[test]
fn match_option_non_exhaustive_pushes_error() {
    let src = "enum Option<T> { Some(T), None }\nmain = () => { let m = Option::Some(42); match m { Option::Some(v) => v } }";
    let mut tc = typecheck(src);
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

#[test]
fn infer_type_args_for_generic_call() {
    let tc = typecheck("fn id<T>(x: T) -> T { x }\nmain = () => id(42)");
    assert!(!tc.call_type_args.is_empty(), "should have inferred type args for id(42)");
    let args = tc.call_type_args.values().next().unwrap();
    assert_eq!(args.len(), 1, "id should have 1 type param");
    assert_eq!(args[0], HirType::Int, "T should be inferred as Int from arg 42");
}

#[test]
fn infer_type_args_for_generic_struct_lit() {
    let tc = typecheck("struct Container<T> { value: T }\nmain = () => { let c = Container { value: 42 }; c.value }");
    assert!(tc.errors.is_empty(), "generic struct lit should not error: {:?}", tc.errors);
}

#[test]
fn call_with_wrong_argument_type_reports_error() {
    let tc = typecheck("fn take_bool(b: bool) -> i64 { 0 }\nmain = () => take_bool(42)");
    assert!(!tc.errors.is_empty(), "should report type error for passing i64 to bool parameter");
    let mismatches: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::MismatchedTypes { .. })).collect();
    assert_eq!(mismatches.len(), 1, "expected exactly one MismatchedTypes error");
}

#[test]
fn let_annotation_mismatch_reports_error() {
    let tc = typecheck("fn main() -> i64 { let x: f64 = 42; x }");
    let mismatches: Vec<_> = tc.errors.iter().filter(|e| matches!(e, TypeError::MismatchedTypes { .. })).collect();
    assert_eq!(mismatches.len(), 1, "expected one MismatchedTypes for annotation vs value");
}

#[test]
fn generic_equality_compiles() {
    let tc = typecheck("fn eq<K>(a: K, b: K) -> bool { a == b }\nmain = () => { if eq(42, 42) { 1 } else { 0 } }");
    if !tc.errors.is_empty() {
        eprintln!("Generic equality errors: {:?}", tc.errors);
    }
    assert!(tc.errors.is_empty(), "generic equality should compile");
}

// ---- NEW TESTS (added once) ----
#[test]
fn extract_option_inner_some() {
    let tc = TypeChecker::new(Interner::new());
    let ty = HirType::Option(Box::new(HirType::Int));
    let inner = tc.extract_option_inner(&ty);
    assert_eq!(inner, Some(HirType::Int));
}

#[test]
fn extract_option_inner_none_for_non_option() {
    let tc = TypeChecker::new(Interner::new());
    assert_eq!(tc.extract_option_inner(&HirType::Int), None);
}

#[test]
fn extract_result_inner_ok() {
    let tc = TypeChecker::new(Interner::new());
    let ty = HirType::Result(Box::new(HirType::Int), Box::new(HirType::Str));
    let (ok, err) = tc.extract_result_inner(&ty).unwrap();
    assert_eq!(ok, HirType::Int);
    assert_eq!(err, HirType::Str);
}

#[test]
fn extract_result_inner_none() {
    let tc = TypeChecker::new(Interner::new());
    assert_eq!(tc.extract_result_inner(&HirType::Int), None);
}

#[test]
fn register_visibility_separate_pub_priv() {
    let mut tc = TypeChecker::new(Interner::new());
    let pub_sym = tc.interner.intern("pub_fn");
    let priv_sym = tc.interner.intern("priv_fn");
    tc.register_visibility(pub_sym, true);
    tc.register_visibility(priv_sym, false);
    assert_eq!(tc.visibility.get(&pub_sym), Some(&true));
    assert_eq!(tc.visibility.get(&priv_sym), Some(&false));
}

#[test]
fn check_fn_generic_return_matches_inferred() {
    let mut tc = TypeChecker::new(Interner::new());
    let fn_sym = tc.interner.intern("id");
    let t_sym = tc.interner.intern("T");
    let f = HirFn {
        doc: None, name: fn_sym, type_params: vec![t_sym],
        params: vec![(tc.interner.intern("x"), HirType::Named(t_sym))],
        param_mutability: vec![false],
        ret: Some(HirType::Named(t_sym)),
        body: HirExpr::Ident { id: ExprId::new(0), name: tc.interner.intern("x"), span: Span::new(0,0) },
        span: Span::new(0,0), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    };
    tc.fns.push(f);
    let f_clone = tc.fns.last().unwrap().clone();
    tc.check_fn(&f_clone);
    assert!(tc.errors.is_empty(), "Expected no errors, got {:?}", tc.errors);
}

#[test]
fn check_fn_return_mismatch_error() {
    let mut tc = TypeChecker::new(Interner::new());
    let f = HirFn {
        doc: None, name: tc.interner.intern("bad"), type_params: vec![], params: vec![],
        param_mutability: vec![], ret: Some(HirType::Bool),
        body: HirExpr::IntLit { id: ExprId::new(0), value: 42, span: Span::new(0,0) },
        span: Span::new(0,0), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    };
    tc.fns.push(f);
    let f_clone = tc.fns.last().unwrap().clone();
    tc.check_fn(&f_clone);
    assert!(tc.errors.iter().any(|e| matches!(e, TypeError::InvalidReturnType { .. })));
}

#[test]
fn bind_match_option_some_pattern() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let v_sym = tc.interner.intern("v");
    let pattern = HirPattern::OptionSome(Box::new(HirPattern::Var(v_sym)));
    let scrutinee_ty = HirType::Option(Box::new(HirType::Int));
    tc.bind_match_pattern(&pattern, &scrutinee_ty);
    assert_eq!(tc.lookup_binding(&v_sym), Some(HirType::Int));
}

#[test]
fn bind_match_option_none_no_binding() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    tc.bind_match_pattern(&HirPattern::OptionNone, &HirType::Option(Box::new(HirType::Int)));
    let v_sym = tc.interner.intern("v");
    assert!(tc.lookup_binding(&v_sym).is_none());
}

#[test]
fn bind_match_result_ok_pattern() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let v_sym = tc.interner.intern("v");
    let pattern = HirPattern::ResultOk(Box::new(HirPattern::Var(v_sym)));
    let scrutinee_ty = HirType::Result(Box::new(HirType::Int), Box::new(HirType::Str));
    tc.bind_match_pattern(&pattern, &scrutinee_ty);
    assert_eq!(tc.lookup_binding(&v_sym), Some(HirType::Int));
}

#[test]
fn bind_match_result_err_pattern() {
    let mut tc = TypeChecker::new(Interner::new());
    tc.push_scope();
    let e_sym = tc.interner.intern("e");
    let pattern = HirPattern::ResultErr(Box::new(HirPattern::Var(e_sym)));
    let scrutinee_ty = HirType::Result(Box::new(HirType::Int), Box::new(HirType::Str));
    tc.bind_match_pattern(&pattern, &scrutinee_ty);
    assert_eq!(tc.lookup_binding(&e_sym), Some(HirType::Str));
}

#[test]
fn unify_int_with_type_param() {
    let mut i = Interner::new();
    let t = i.intern("T");
    let result = unify::unify(&HirType::Int, &HirType::Named(t), &[t]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_int_with_bool_fails() {
    let result = unify::unify(&HirType::Int, &HirType::Bool, &[]);
    assert!(result.is_err());
}

#[test]
fn unify_rawptr_success() {
    let mut i = Interner::new();
    let t = i.intern("T");
    let result = unify::unify(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &[t],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_option_success() {
    let mut i = Interner::new();
    let t = i.intern("T");
    let result = unify::unify(
        &HirType::Option(Box::new(HirType::Int)),
        &HirType::Option(Box::new(HirType::Named(t))),
        &[t],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()[&t], HirType::Int);
}

#[test]
fn unify_result_success() {
    let mut i = Interner::new();
    let t = i.intern("T");
    let e = i.intern("E");
    let result = unify::unify(
        &HirType::Result(Box::new(HirType::Int), Box::new(HirType::Str)),
        &HirType::Result(Box::new(HirType::Named(t)), Box::new(HirType::Named(e))),
        &[t, e],
    );
    assert!(result.is_ok());
    let sub = result.unwrap();
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&e], HirType::Str);
}

#[test]
fn unify_tuple_mismatched_length_fails() {
    let result = unify::unify(
        &HirType::Tuple(vec![HirType::Int]),
        &HirType::Tuple(vec![HirType::Int, HirType::Bool]),
        &[],
    );
    assert!(result.is_err());
}

#[test]
fn is_valid_cast_float_to_int_valid() {
    assert!(resolver::is_valid_cast(&HirType::Float, &HirType::Int));
}

#[test]
fn is_valid_cast_str_to_float_invalid() {
    assert!(!resolver::is_valid_cast(&HirType::Str, &HirType::Float));
}

#[test]
fn is_valid_cast_any_to_named_valid() {
    let mut i = Interner::new();
    let sym = i.intern("SomeStruct");
    assert!(resolver::is_valid_cast(&HirType::Int, &HirType::Named(sym)));
}

#[test]
fn unify_types_rawptr_success() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let type_params = vec![t];
    let mut sub = std::collections::HashMap::new();
    TypeChecker::unify_types(
        &HirType::RawPtr(Box::new(HirType::Int)),
        &HirType::RawPtr(Box::new(HirType::Named(t))),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
}

#[test]
fn unify_types_nested_generics() {
    let mut tc = TypeChecker::new(Interner::new());
    let t = tc.interner.intern("T");
    let u = tc.interner.intern("U");
    let type_params = vec![t, u];
    let mut sub = std::collections::HashMap::new();
    TypeChecker::unify_types(
        &HirType::Generic(tc.interner.intern("Pair"), vec![HirType::Int, HirType::Bool]),
        &HirType::Generic(tc.interner.intern("Pair"), vec![HirType::Named(t), HirType::Named(u)]),
        &type_params,
        &mut sub,
    );
    assert_eq!(sub[&t], HirType::Int);
    assert_eq!(sub[&u], HirType::Bool);
}

#[test]
fn infer_deref_rawptr_type() {
    let mut tc = TypeChecker::new(Interner::new());
    let ptr_sym = tc.interner.intern("p");
    tc.push_scope();
    tc.insert_binding(ptr_sym, HirType::RawPtr(Box::new(HirType::Int)), false);
    let deref_expr = HirExpr::Deref {
        id: ExprId::new(0),
        expr: Box::new(HirExpr::Ident { id: ExprId::new(1), name: ptr_sym, span: Span::new(0,0) }),
        span: Span::new(0,0),
    };
    assert_eq!(tc.check_expr(&deref_expr), Some(HirType::Int));
}

#[test]
fn infer_generic_call_sets_type_args() {
    let mut tc = TypeChecker::new(Interner::new());
    let fn_sym = tc.interner.intern("id");
    let t_sym = tc.interner.intern("T");
    tc.fns.push(HirFn {
        doc: None, name: fn_sym, type_params: vec![t_sym],
        params: vec![(tc.interner.intern("x"), HirType::Named(t_sym))],
        param_mutability: vec![false],
        ret: Some(HirType::Named(t_sym)),
        body: HirExpr::IntLit { id: ExprId::new(99), value: 0, span: Span::new(0,0) },
        span: Span::new(0,0), is_pub: false, is_macro_generated: false, is_extern_backed: false,
    });
    let call = HirExpr::Call {
        id: ExprId::new(2),
        callee: fn_sym,
        args: vec![HirExpr::IntLit { id: ExprId::new(3), value: 42, span: Span::new(0,0) }],
        span: Span::new(0,0),
    };
    assert_eq!(tc.check_expr(&call), Some(HirType::Int));
    assert!(tc.call_type_args.contains_key(&ExprId::new(2)));
    assert_eq!(tc.call_type_args[&ExprId::new(2)], vec![HirType::Int]);
}

#[test]
fn infer_struct_lit_missing_field_error() {
    let mut tc = TypeChecker::new(Interner::new());
    let point_sym = tc.interner.intern("Point");
    let x_sym = tc.interner.intern("x");
    let y_sym = tc.interner.intern("y");
    tc.structs.insert(point_sym, crate::typeck::StructInfo {
        fields: vec![
            glyim_hir::item::StructField { name: x_sym, ty: HirType::Int, doc: None },
            glyim_hir::item::StructField { name: y_sym, ty: HirType::Int, doc: None },
        ],
        field_map: {
            let mut m = std::collections::HashMap::new();
            m.insert(x_sym, 0);
            m.insert(y_sym, 1);
            m
        },
        type_params: vec![],
    });
    let lit = HirExpr::StructLit {
        id: ExprId::new(0),
        struct_name: point_sym,
        fields: vec![(x_sym, HirExpr::IntLit { id: ExprId::new(1), value: 1, span: Span::new(0,0) })],
        span: Span::new(0,0),
    };
    tc.check_expr(&lit);
    assert!(tc.errors.iter().any(|e| matches!(e, TypeError::MissingField { .. })), "Expected MissingField error, got {:?}", tc.errors);
}
