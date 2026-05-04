use crate::HirItem;
use crate::{Hir, HirExpr, HirStmt};

fn lower_source(source: &str) -> (Hir, glyim_interner::Interner) {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        panic!("parse errors: {:?}", parse_out.errors);
    }
    let mut interner = parse_out.interner;
    let hir = crate::lower(&parse_out.ast, &mut interner);
    (hir, interner)
}

fn get_main_body<'a>(hir: &'a Hir, interner: &'a glyim_interner::Interner) -> &'a HirExpr {
    for item in &hir.items {
        if let HirItem::Fn(f) = item {
            if interner.resolve(f.name) == "main" {
                return &f.body;
            }
        }
    }
    panic!("no function named 'main' found");
}

/// Extract the "effective value" from a body expression.
/// If it's a block, return the last expression (or a dummy).
/// Otherwise return the expression itself.
fn expr_value<'a>(expr: &'a HirExpr) -> &'a HirExpr {
    match expr {
        HirExpr::Block { stmts, .. } => stmts
            .iter()
            .rev()
            .find_map(|s| match s {
                HirStmt::Expr(e) => Some(e),
                _ => None,
            })
            .unwrap_or(expr),
        other => other,
    }
}

// ---- Expression lowering ----
#[test]
fn lower_int_lit() {
    let (hir, mut interner) = lower_source("main = () => 42");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert_eq!(
        val,
        &HirExpr::IntLit {
            id: val.get_id(),
            value: 42,
            span: val.get_span()
        }
    );
    let sym = interner.intern("test");
    assert_eq!(interner.resolve(sym), "test");
}

#[test]
fn lower_float_lit() {
    let (hir, interner) = lower_source("main = () => 3.14");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::FloatLit { value: f, .. } => assert!((f - 3.14).abs() < 1e-6),
        _ => panic!("expected FloatLit"),
    }
}

#[test]
fn lower_bool_lit() {
    let (hir, interner) = lower_source("main = () => true");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::BoolLit { value: true, .. } => {}
        _ => panic!("expected BoolLit true"),
    }
}

#[test]
fn lower_str_lit() {
    let (hir, interner) = lower_source(r#"main = () => "hello""#);
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::StrLit { value: s, .. } => assert_eq!(s, r#""hello""#),
        _ => panic!("expected StrLit"),
    }
}

#[test]
fn lower_binary_expr() {
    let (hir, interner) = lower_source("main = () => 1 + 2");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(
        val,
        HirExpr::Binary {
            op: crate::HirBinOp::Add,
            ..
        }
    ));
}

#[test]
fn lower_unary_neg() {
    let (hir, interner) = lower_source("main = () => -5");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(
        val,
        HirExpr::Unary {
            op: crate::HirUnOp::Neg,
            ..
        }
    ));
}

#[test]
fn lower_unary_not() {
    let (hir, interner) = lower_source("main = () => !true");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(
        val,
        HirExpr::Unary {
            op: crate::HirUnOp::Not,
            ..
        }
    ));
}

#[test]
fn lower_ident() {
    let (hir, mut interner) = lower_source("main = () => { let x = 42; x }");
    let x_sym = interner.intern("x");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::Ident { name, .. } => assert_eq!(*name, x_sym),
        _ => panic!("expected Ident"),
    }
}

// ---- Statement lowering ----
#[test]
fn lower_let_stmt() {
    let (hir, interner) = lower_source("main = () => { let x = 42 }");
    let body = get_main_body(&hir, &interner);
    match body {
        HirExpr::Block { stmts, .. } => {
            assert!(!stmts.is_empty());
            match &stmts[0] {
                HirStmt::LetPat {
                    pattern, mutable, ..
                } => {
                    assert_eq!(*mutable, false);
                    match pattern {
                        crate::HirPattern::Var(name) => assert_eq!(interner.resolve(*name), "x"),
                        _ => panic!("expected Var pattern"),
                    }
                }
                _ => panic!("expected LetPat"),
            }
        }
        _ => panic!("expected Block"),
    }
}

#[test]
fn lower_let_mut_stmt() {
    let (hir, interner) = lower_source("main = () => { let mut x = 10 }");
    let body = get_main_body(&hir, &interner);
    match body {
        HirExpr::Block { stmts, .. } => match &stmts[0] {
            HirStmt::LetPat {
                pattern, mutable, ..
            } => {
                assert_eq!(*mutable, true);
                match pattern {
                    crate::HirPattern::Var(name) => assert_eq!(interner.resolve(*name), "x"),
                    _ => panic!("expected Var pattern"),
                }
            }
            _ => panic!("expected mut LetPat"),
        },
        _ => panic!("expected Block"),
    }
}

#[test]
fn lower_assign_stmt() {
    let (hir, interner) = lower_source("main = () => { let mut x = 10\nx = x + 5 }");
    let body = get_main_body(&hir, &interner);
    match body {
        HirExpr::Block { stmts, .. } => {
            let assign = stmts.iter().find(|s| matches!(s, HirStmt::Assign { .. }));
            assert!(assign.is_some());
            if let Some(HirStmt::Assign { target, .. }) = assign {
                assert_eq!(interner.resolve(*target), "x");
            }
        }
        _ => panic!("expected Block"),
    }
}

// ---- Control flow ----
#[test]
fn lower_if_without_else() {
    let (hir, interner) = lower_source("main = () => { if 1 { 42 } }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(
        matches!(val, HirExpr::If { .. }),
        "expected If, got {:?}",
        val
    );
}

#[test]
fn lower_if_with_else() {
    let (hir, interner) = lower_source("main = () => { if 0 { 10 } else { 20 } }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(val, HirExpr::If { .. }));
}

#[test]
fn lower_match_basic() {
    let (hir, interner) = lower_source("main = () => { match 1 { 1 => 10, _ => 20 } }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(val, HirExpr::Match { .. }));
}

#[test]
fn lower_match_with_enum_patterns() {
    let (hir, mut interner) = lower_source(
        "enum Color { Red, Green }\nmain = () => { let c = Color::Red; match c { Color::Red => 1, Color::Green => 2 } }",
    );
    let red_sym = interner.intern("Red");
    let green_sym = interner.intern("Green");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    if let HirExpr::Match { arms, .. } = val {
        assert_eq!(arms.len(), 2);
        let has_red = arms.iter().any(|arm| matches!(arm.pattern, crate::HirPattern::EnumVariant { variant_name, .. } if variant_name == red_sym));
        let has_green = arms.iter().any(|arm| matches!(arm.pattern, crate::HirPattern::EnumVariant { variant_name, .. } if variant_name == green_sym));
        assert!(has_red && has_green, "arms missing expected variants");
    } else {
        panic!("expected Match, got {:?}", val);
    }
}

#[test]
fn lower_else_if_chain() {
    let (hir, interner) = lower_source("main = () => { if 0 { 1 } else if 0 { 2 } else { 3 } }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    let source = format!("{:?}", val);
    let if_count = source.matches("If").count();
    assert!(if_count >= 2, "expected nested Ifs");
}

// ---- Item lowering ----
#[test]
fn lower_fn_def_with_params() {
    let (hir, interner) = lower_source("fn add(a, b) { a + b }\nmain = () => add(1, 2)");
    let add_fn = hir
        .items
        .iter()
        .find(|item| matches!(item, HirItem::Fn(f) if interner.resolve(f.name) == "add"));
    assert!(add_fn.is_some());
    if let Some(HirItem::Fn(f)) = add_fn {
        assert_eq!(f.params.len(), 2);
    }
}

#[test]
fn lower_struct_def() {
    let (hir, interner) = lower_source("struct Point { x, y }");
    let s = hir.items.iter().find_map(|i| {
        if let HirItem::Struct(s) = i {
            Some(s)
        } else {
            None
        }
    });
    assert!(s.is_some());
    let s = s.unwrap();
    assert_eq!(interner.resolve(s.name), "Point");
    assert_eq!(s.fields.len(), 2);
}

#[test]
fn lower_enum_def() {
    let (hir, interner) = lower_source("enum Color { Red, Green, Blue }");
    let e = hir.items.iter().find_map(|i| {
        if let HirItem::Enum(e) = i {
            Some(e)
        } else {
            None
        }
    });
    assert!(e.is_some());
    let e = e.unwrap();
    assert_eq!(interner.resolve(e.name), "Color");
    assert_eq!(e.variants.len(), 3);
}

#[test]
fn lower_impl_block_mangles_methods() {
    let (hir, interner) = lower_source(
        "struct Point { x, y }\nimpl Point {\n    fn zero() -> Point { Point { x: 0, y: 0 } }\n}\nmain = () => 0",
    );
    let imp = hir.items.iter().find_map(|i| {
        if let HirItem::Impl(imp) = i {
            Some(imp)
        } else {
            None
        }
    });
    assert!(imp.is_some());
    let imp = imp.unwrap();
    assert_eq!(imp.methods.len(), 1);
    assert_eq!(interner.resolve(imp.methods[0].name), "Point_zero");
}

#[test]
fn lower_extern_block() {
    let (hir, _) = lower_source(
        "extern {\n    fn write(fd: i64, buf: *mut u8, len: i64) -> i64\n}\nmain = () => 0",
    );
    let ext = hir.items.iter().find(|i| matches!(i, HirItem::Extern(_)));
    assert!(ext.is_some());
}

// ---- Sugar lowering ----
#[test]
fn lower_some_expr() {
    let (hir, interner) = lower_source("main = () => Some(42)");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::EnumVariant { variant_name, .. } if interner.resolve(*variant_name) == "Some" => {}
        _ => panic!("expected Some enum variant, got {:?}", val),
    }
}

#[test]
fn lower_none_expr() {
    let (hir, interner) = lower_source("main = () => None");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    match val {
        HirExpr::EnumVariant {
            variant_name, args, ..
        } if interner.resolve(*variant_name) == "None" && args.is_empty() => {}
        _ => panic!("expected None variant, got {:?}", val),
    }
}

#[test]
fn lower_ok_err_expr() {
    let (hir, interner) = lower_source("main = () => { let r = Ok(42); let e = Err(0); 0 }");
    let body = get_main_body(&hir, &interner);
    let let_stmts: Vec<_> = match body {
        HirExpr::Block { stmts, .. } => stmts
            .iter()
            .filter_map(|s| {
                if let HirStmt::LetPat { value, .. } = s {
                    Some(value)
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    };
    let has_ok = let_stmts.iter().any(|v| matches!(v, HirExpr::EnumVariant { variant_name, .. } if interner.resolve(*variant_name) == "Ok"));
    let has_err = let_stmts.iter().any(|v| matches!(v, HirExpr::EnumVariant { variant_name, .. } if interner.resolve(*variant_name) == "Err"));
    assert!(has_ok && has_err, "expected Ok and Err");
}

#[test]
fn lower_try_expr_desugars_to_match() {
    let (hir, interner) = lower_source("main = () => { let r = Ok(42)?; r }");
    let body = get_main_body(&hir, &interner);
    match body {
        HirExpr::Block { stmts, .. } => {
            assert!(
                stmts.iter().any(|s| matches!(
                    s,
                    HirStmt::LetPat {
                        value: HirExpr::Match { .. },
                        ..
                    }
                )),
                "expected Match to appear as right-hand side of a let binding"
            );
        }
        _ => panic!("expected Block, got {:?}", body),
    }
}

#[test]
fn lower_struct_literal() {
    let (hir, interner) = lower_source("struct Point { x, y }\nmain = () => Point { x: 1, y: 2 }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(val, HirExpr::StructLit { .. }));
}

#[test]
fn lower_field_access() {
    let (hir, interner) =
        lower_source("struct Point { x, y }\nmain = () => { let p = Point { x: 1, y: 2 }; p.x }");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(val, HirExpr::FieldAccess { .. }));
}

#[test]
fn lower_as_expr() {
    let (hir, interner) = lower_source("main = () => 42 as f64");
    let body = get_main_body(&hir, &interner);
    let val = expr_value(body);
    assert!(matches!(val, HirExpr::As { .. }));
}

#[test]
fn expr_ids_are_monotonic() {
    let (hir, interner) = lower_source("main = () => { let x = 1; let y = 2; x + y }");
    let body = get_main_body(&hir, &interner);
    fn collect_ids(expr: &HirExpr, ids: &mut Vec<u32>) {
        ids.push(expr.get_id().as_usize() as u32);
        match expr {
            HirExpr::Block { stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        HirStmt::Expr(e) => collect_ids(e, ids),
                        HirStmt::Let { value, .. } | HirStmt::Assign { value, .. } => {
                            collect_ids(value, ids)
                        }
                        _ => {}
                    }
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                collect_ids(lhs, ids);
                collect_ids(rhs, ids);
            }
            HirExpr::Unary { operand, .. } => collect_ids(operand, ids),
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                collect_ids(condition, ids);
                collect_ids(then_branch, ids);
                if let Some(e) = else_branch {
                    collect_ids(e, ids);
                }
            }
            _ => {}
        }
    }
    let mut ids = Vec::new();
    collect_ids(body, &mut ids);
    let mut last_id: i32 = -1;
    for &id in &ids {
        assert!(
            id as i32 > last_id,
            "ExprId not monotonic: {} after {}",
            id,
            last_id
        );
        last_id = id as i32;
    }
}

#[test]
fn lower_empty_source() {
    let (hir, _) = lower_source("");
    assert!(hir.items.is_empty());
}
#[test]
fn lower_enum_variant_construction() {
    let (hir, interner) =
        lower_source("enum Color { Red, Green }\nmain = () => { let c = Color::Green; c }");
    let body = get_main_body(&hir, &interner);
    let has_enum = match body {
        HirExpr::Block { stmts, .. } => stmts.iter().any(|s| {
            matches!(
                s,
                HirStmt::LetPat {
                    value: HirExpr::EnumVariant { .. },
                    ..
                }
            )
        }),
        _ => false,
    };
    assert!(has_enum, "expected EnumVariant somewhere");
}
#[test]
fn lower_struct_preserves_type_params() {
    let (hir, mut interner) = lower_source("struct Container<T> { value: T }\nmain = () => 0");
    let s = hir.items.iter().find_map(|i| {
        if let HirItem::Struct(s) = i {
            Some(s)
        } else {
            None
        }
    });
    assert!(s.is_some(), "expected Struct item");
    let s = s.unwrap();
    let t_sym = interner.intern("T");
    assert_eq!(
        s.type_params,
        vec![t_sym],
        "struct type_params should be preserved"
    );
}
#[test]
fn lower_enum_preserves_type_params() {
    let (hir, mut interner) = lower_source("enum Option<T> { Some(T), None }\nmain = () => 0");
    let e = hir.items.iter().find_map(|i| {
        if let HirItem::Enum(e) = i {
            Some(e)
        } else {
            None
        }
    });
    assert!(e.is_some(), "expected Enum item");
    let e = e.unwrap();
    let t_sym = interner.intern("T");
    assert_eq!(
        e.type_params,
        vec![t_sym],
        "enum type_params should be preserved"
    );
}
