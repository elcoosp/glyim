use crate::pipeline_api::lower_crate_for_pipeline;
use crate::{BodyId, CrateHir, Expr, ExprId, ItemKind, Pat};
use glyim_core::interner::Interner;
use glyim_test::phase::FrontendTester;

struct TestContext {
    hir: CrateHir,
    interner: Interner,
}

fn lower_source(source: &str) -> TestContext {
    let trace = FrontendTester::new(source).run();
    let root = trace.parse_tree.unwrap();
    let mut interner = Interner::new();
    let (hir, diags) = lower_crate_for_pipeline(&root, &mut interner);
    assert!(
        diags.is_empty(),
        "Lowering produced diagnostics: {:?}",
        diags
    );
    TestContext { hir, interner }
}

fn find_fn_body(ctx: &TestContext, fn_name: &str) -> Option<BodyId> {
    let fn_name_name = ctx.interner.intern(fn_name);
    for item in ctx.hir.items.iter() {
        if let ItemKind::Fn(fn_item) = &item.kind
            && item.name == fn_name_name
        {
            return fn_item.body;
        }
    }
    None
}

fn find_expr_in_body(
    hir: &CrateHir,
    body_id: BodyId,
    pred: impl Fn(&Expr) -> bool,
) -> Option<ExprId> {
    let body = &hir.bodies[body_id];
    for (id, expr) in body.exprs.iter_enumerated() {
        if pred(expr) {
            return Some(id);
        }
    }
    None
}

#[test]
fn test_for_loop_lowering() {
    let ctx = lower_source("fn test() { for i in 0..10 { } }");
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let for_expr_id = find_expr_in_body(&ctx.hir, body_id, |e| matches!(e, Expr::For { .. }));
    assert!(for_expr_id.is_some(), "For expression not found");

    let body = &ctx.hir.bodies[body_id];
    if let Expr::For {
        pat,
        iterable,
        body: body_expr,
    } = &body.exprs[for_expr_id.unwrap()]
    {
        let pat_node = &body.pats[*pat];
        let i_name = ctx.interner.intern("i");
        assert!(matches!(pat_node, Pat::Binding { name, .. } if *name == i_name));
        assert!(matches!(&body.exprs[*iterable], Expr::Range { .. }));
        assert!(matches!(&body.exprs[*body_expr], Expr::Block { .. }));
    } else {
        panic!("Expr is not For");
    }
}

#[test]
fn test_range_expression_lowering() {
    let ctx = lower_source("fn test() { let _ = 1..=5; }");
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let range_expr_id = find_expr_in_body(&ctx.hir, body_id, |e| matches!(e, Expr::Range { .. }));
    assert!(range_expr_id.is_some(), "Range expression not found");

    let body = &ctx.hir.bodies[body_id];
    if let Expr::Range {
        start,
        end,
        inclusive,
    } = &body.exprs[range_expr_id.unwrap()]
    {
        assert!(start.is_some(), "Range start missing");
        assert!(end.is_some(), "Range end missing");
        assert!(*inclusive, "Range should be inclusive");
    } else {
        panic!("Expr is not Range");
    }
}

#[test]
fn test_struct_expr_lowering() {
    let source = r#"
        struct Point { x: i32, y: i32 }
        fn test() { let p = Point { x: 10, y: 20 }; }
    "#;
    let ctx = lower_source(source);
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let struct_expr_id = find_expr_in_body(&ctx.hir, body_id, |e| matches!(e, Expr::Struct { .. }));
    assert!(struct_expr_id.is_some(), "Struct expression not found");

    let body = &ctx.hir.bodies[body_id];
    if let Expr::Struct {
        path,
        fields,
        spread,
    } = &body.exprs[struct_expr_id.unwrap()]
    {
        let name = path
            .as_name()
            .expect("Struct path should be single segment");
        let point_name = ctx.interner.intern("Point");
        assert_eq!(name, point_name);
        assert_eq!(fields.len(), 2, "Expected 2 fields");
        assert!(spread.is_none(), "Spread should be None");
        let x_name = ctx.interner.intern("x");
        let y_name = ctx.interner.intern("y");
        assert_eq!(fields[0].0, x_name);
        assert_eq!(fields[1].0, y_name);
    } else {
        panic!("Expr is not Struct");
    }
}

// ===================== §3.4: missing-field diagnostics =====================

use glyim_diag::GlyimDiagnostic;

/// Like `lower_source` but returns the diagnostics instead of asserting empty.
fn lower_source_diags(source: &str) -> (CrateHir, Interner, Vec<GlyimDiagnostic>) {
    let trace = FrontendTester::new(source).run();
    let root = trace.parse_tree.unwrap();
    let mut interner = Interner::new();
    let (hir, diags) = lower_crate_for_pipeline(&root, &mut interner);
    (hir, interner, diags)
}

fn has_missing_field_error(diags: &[GlyimDiagnostic]) -> bool {
    diags.iter().any(|d| d.message.contains("missing field(s)"))
}

#[test]
fn missing_field_without_spread_is_error() {
    let source = r#"
        struct Point { x: i32, y: i32 }
        fn test() { let p = Point { x: 10 }; }
    "#;
    let (_hir, _interner, diags) = lower_source_diags(source);
    assert!(
        has_missing_field_error(&diags),
        "omitting field `y` without `..base` must be a hard error; diags={:?}",
        diags
    );
    // The diagnostic must name the missing field(s).
    assert!(
        diags.iter().any(|d| d.message.contains("y")),
        "diagnostic should name the missing field `y`; diags={:?}",
        diags
    );
}

#[test]
fn missing_field_with_spread_compiles() {
    let source = r#"
        struct Point { x: i32, y: i32 }
        fn test() {
            let base = Point { x: 1, y: 2 };
            let p = Point { x: 10, ..base };
        }
    "#;
    let (_hir, _interner, diags) = lower_source_diags(source);
    assert!(
        !has_missing_field_error(&diags),
        "missing field with `..base` must NOT be an error; diags={:?}",
        diags
    );
}

#[test]
fn missing_field_lists_all_missing_at_once() {
    let source = r#"
        struct Triple { a: i32, b: i32, c: i32 }
        fn test() { let p = Triple { a: 1 }; }
    "#;
    let (_hir, _interner, diags) = lower_source_diags(source);
    assert!(
        has_missing_field_error(&diags),
        "omitting `b` and `c` must be an error; diags={:?}",
        diags
    );
    // Both missing fields must be reported in a single diagnostic.
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("b") && d.message.contains("c")),
        "diagnostic should list ALL missing fields (b, c); diags={:?}",
        diags
    );
}

// ===================== §session-3: byte literals & arena completeness =====

/// Regression for the session-3 bug: `b'0'` (a `SyntaxKind::ByteLit`) is a
/// `u8` literal, and must survive HIR lowering as a real `Expr::Literal`.
/// Before the fix, `SyntaxKind::is_literal()` omitted `ByteLit`, so
/// `lower_lit_expr` returned `None` and the enclosing statement vanished from
/// the HIR (surfacing later as `unresolved name` for whatever the dropped
/// `let` was meant to bind).
#[test]
fn byte_literal_lowers_as_u8_literal() {
    let ctx = lower_source("fn test() { let a: u8 = b'0'; let _ = a; }");
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let lit_id = find_expr_in_body(&ctx.hir, body_id, |e| {
        matches!(
            e,
            Expr::Literal(crate::Literal::Uint(48, Some(glyim_core::primitives::UintTy::U8)))
        )
    });
    assert!(
        lit_id.is_some(),
        "b'0' must lower to Literal::Uint(48, Some(U8)); body dump: {:?}",
        &ctx.hir.bodies[body_id].exprs
    );
}

/// Byte literal inside a cast — the exact minimal shape from the session-3
/// handoff (`let digit: i32 = (ch as i32) - (b'0' as i32);`). Verifies the
/// HIR contains the `let`'s `Expr::Let` and the embedded byte literal.
#[test]
fn byte_literal_inside_cast_lowers_the_let_statement() {
    let ctx = lower_source(
        "fn test(ch: u8) -> i32 { let digit: i32 = (ch as i32) - (b'0' as i32); digit }",
    );
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let body = &ctx.hir.bodies[body_id];

    // The let must be present as Expr::Let.
    let has_let = body.exprs.iter().any(|e| matches!(e, Expr::Let { .. }));
    assert!(
        has_let,
        "`let digit: i32 = (ch as i32) - (b'0' as i32);` must lower to Expr::Let; body dump: {:?}",
        body.exprs
    );
    // The byte literal must be present.
    let has_byte = body.exprs.iter().any(|e| {
        matches!(
            e,
            Expr::Literal(crate::Literal::Uint(48, Some(glyim_core::primitives::UintTy::U8)))
        )
    });
    assert!(
        has_byte,
        "the byte literal `b'0'` must be present in the HIR; body dump: {:?}",
        body.exprs
    );
}

/// Kitchen-sink coverage: a body that mixes every statement kind the
/// block-lowering must handle. If any leaf function silently returns `None`
/// for one of these constructs, the statement vanishes from the HIR and
/// *this* test's arena-size assertion catches it at the lowering boundary
/// (before it can surface as a phantom `unresolved name` far downstream).
///
/// The exact counts are not the point — the point is that every distinct
/// statement kind shows up. Adjust the expected counts if the block/expr
/// lowering legitimately changes (e.g. adds a desugaring pass).
#[test]
fn kitchen_sink_body_lowers_all_statements() {
    let source = r#"
        fn clamp(x: i32, hi: i32) -> i32 {
            if x > hi { hi } else { x }
        }
        fn test(ch: u8, s: &str) -> i32 {
            let bytes = s.as_bytes();
            let mut i = 0;
            let mut value: i32 = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b < b'0' || b > b'9' {
                    return -1;
                }
                let digit: i32 = (b as i32) - (b'0' as i32);
                value = value * 10 + digit;
                i += 1;
            }
            clamp(value, 100)
        }
    "#;
    let ctx = lower_source(source);
    let body_id = find_fn_body(&ctx, "test").expect("Function test not found");
    let body = &ctx.hir.bodies[body_id];

    // Statement-level constructs that must have survived:
    let has_let = body.exprs.iter().any(|e| matches!(e, Expr::Let { .. }));
    let has_while = body.exprs.iter().any(|e| matches!(e, Expr::While { .. }));
    let has_if = body.exprs.iter().any(|e| matches!(e, Expr::If { .. }));
    let has_assign = body.exprs.iter().any(|e| matches!(e, Expr::Assign { .. }));
    let has_return = body.exprs.iter().any(|e| matches!(e, Expr::Return { .. }));
    let has_cast = body.exprs.iter().any(|e| matches!(e, Expr::Cast { .. }));
    let has_binary = body.exprs.iter().any(|e| matches!(e, Expr::Binary { .. }));
    let has_call = body.exprs.iter().any(|e| matches!(e, Expr::Call { .. }));
    let has_index = body.exprs.iter().any(|e| matches!(e, Expr::Index { .. }));
    let has_method = body.exprs.iter().any(|e| matches!(e, Expr::MethodCall { .. }));
    let has_byte_lit = body.exprs.iter().any(|e| {
        matches!(
            e,
            Expr::Literal(crate::Literal::Uint(48, Some(glyim_core::primitives::UintTy::U8)))
        )
    });

    // One `let` for each of `bytes`, `i`, `value`, `b`, `digit` (5 total).
    let let_count = body
        .exprs
        .iter()
        .filter(|e| matches!(e, Expr::Let { .. }))
        .count();

    assert!(has_let, "missing Expr::Let; body dump: {:?}", body.exprs);
    assert_eq!(
        let_count, 5,
        "expected exactly 5 `let` bindings to survive lowering; body dump: {:?}",
        body.exprs
    );
    assert!(has_while, "missing Expr::While");
    assert!(has_if, "missing Expr::If");
    assert!(has_assign, "missing Expr::Assign");
    assert!(has_return, "missing Expr::Return");
    assert!(has_cast, "missing Expr::Cast");
    assert!(has_binary, "missing Expr::Binary");
    assert!(has_call, "missing Expr::Call");
    assert!(has_index, "missing Expr::Index");
    assert!(has_method, "missing Expr::MethodCall");
    assert!(
        has_byte_lit,
        "missing byte literal (should appear for b'0' / b'9')"
    );
}
