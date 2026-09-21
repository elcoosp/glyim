use super::test_utils::{empty_def_map, make_ty_ctx};
use crate::tests::test_utils::global_interner;
use crate::thir;
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::*;
use glyim_hir::{Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind, Pat};
use glyim_span::Span;
use glyim_test::{assert_no_errors, mock::MockSolver};

/// Recursively collect every `ExprKind::Closure` in a THIR body.
fn collect_closures(body: &thir::Body) -> Vec<thir::Expr> {
    let mut out = Vec::new();
    for stmt in &body.stmts {
        if let thir::Stmt::Expr { expr } = stmt {
            collect_in_expr(expr, &mut out);
        }
    }
    out
}

fn collect_in_expr(expr: &thir::Expr, out: &mut Vec<thir::Expr>) {
    if let thir::ExprKind::Closure { .. } = expr.kind {
        out.push(expr.clone());
    }
    match &expr.kind {
        thir::ExprKind::Binary { lhs, rhs, .. } => {
            collect_in_expr(lhs, out);
            collect_in_expr(rhs, out);
        }
        thir::ExprKind::Block { stmts, tail } => {
            for s in stmts {
                if let thir::Stmt::Expr { expr } = s {
                    collect_in_expr(expr, out);
                }
            }
            if let Some(t) = tail {
                collect_in_expr(t, out);
            }
        }
        thir::ExprKind::Ref { operand, .. } => collect_in_expr(operand, out),
        _ => {}
    }
}

/// `fn main(x: i32) { |y: i32| x }`
///
/// `x` is an enclosing-scope binding (the outer fn param), so it must be
/// captured by the closure as `ByRef(Not)`. The closure's own param `y`
/// lives above the capture boundary and must NOT be a capture.
///
/// NOTE: this minimal HIR test harness checks every top-level body expr as a
/// statement, so the closure body may only reference names in the *outer*
/// scope (`x`); referencing the closure's own param `y` inside the body would
/// also be checked standalone at the top level and incorrectly fail to
/// resolve. That limitation is orthogonal to capture analysis — the boundary
/// test still holds because `y` (above the boundary) is never captured.
#[test]
fn closure_captures_enclosing_param() {
    let inter = global_interner();
    let main_name = inter.intern("main");
    let x_name = inter.intern("x");
    let y_name = inter.intern("y");
    let i32_name = inter.intern("i32");

    let mut pats: IndexVec<_, Pat> = IndexVec::new();
    let y_pat = pats.push(Pat::Binding {
        name: y_name,
        mutability: Mutability::Not,
        subpattern: None,
    });

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let x_ref = exprs.push(Expr::Path(glyim_hir::Path::from_single(x_name)));
    let _closure = exprs.push(Expr::Closure {
        params: vec![y_pat],
        body: x_ref,
        is_move: false,
    });

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.len()]),
    };
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    let body_id = bodies.push(body);

    let item = Item {
        id: ItemId::from_raw(0),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![glyim_hir::Param {
                name: x_name,
                ty: Some(glyim_hir::TypeRef::Path(glyim_hir::Path::from_single(
                    i32_name,
                ))),
                span: Span::DUMMY,
            }],
            return_ty: None,
            body: Some(body_id),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: Vec::new(),
            abi: None,
        }),
        visibility: Visibility::Public,
        span: Span::DUMMY,
    };

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    items.push(item);
    let mut body_owners = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));
    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = make_ty_ctx();
    let def_map = empty_def_map();
    let mut solver = MockSolver::new().respond_for_any(glyim_solve::SolverResult::Proven);
    let (ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    for d in &result.diagnostics {
        eprintln!("DIAG: {d:?}");
    }
    assert_no_errors(&result.diagnostics);

    let (_, thir_body) = &result.thir_bodies[0];
    let closures = collect_closures(thir_body);
    assert_eq!(closures.len(), 1, "exactly one closure should be produced");

    let (captures, closure_ty) = match &closures[0].kind {
        thir::ExprKind::Closure { captures, .. } => (captures.clone(), closures[0].ty),
        _ => panic!("expected a closure"),
    };
    assert_eq!(
        captures.len(),
        1,
        "closure must capture exactly one variable (x)"
    );
    // The captured kind must be ByRef (immutable), not ByValue/ByMutRef.
    assert_eq!(
        captures[0].kind,
        thir::CaptureKind::ByRef(Mutability::Not),
        "capture of an immutable enclosing binding is ByRef(Not)"
    );
    // Tier 1.1b: the closure type must be a concrete ADT, not an unresolved
    // inference variable.
    assert!(
        matches!(ctx.ty_kind(closure_ty), glyim_type::TyKind::Closure(_, _)),
        "closure type should resolve to a concrete Closure, got {closure_ty:?}"
    );
}

/// §2.2: a `move` closure captures its environment *by value*.
///
/// `fn main(x: i32) { move |y: i32| x }` — `x` is an enclosing binding; with
/// `move` it must be captured as `ByValue` rather than `ByRef(Not)`.
#[test]
fn move_closure_captures_by_value() {
    let inter = global_interner();
    let main_name = inter.intern("main");
    let x_name = inter.intern("x");
    let y_name = inter.intern("y");
    let i32_name = inter.intern("i32");

    let mut pats: IndexVec<_, Pat> = IndexVec::new();
    let y_pat = pats.push(Pat::Binding {
        name: y_name,
        mutability: Mutability::Not,
        subpattern: None,
    });

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let x_ref = exprs.push(Expr::Path(glyim_hir::Path::from_single(x_name)));
    let _closure = exprs.push(Expr::Closure {
        params: vec![y_pat],
        body: x_ref,
        is_move: true,
    });

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.len()]),
    };
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    let body_id = bodies.push(body);

    let item = Item {
        id: ItemId::from_raw(0),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![glyim_hir::Param {
                name: x_name,
                ty: Some(glyim_hir::TypeRef::Path(glyim_hir::Path::from_single(
                    i32_name,
                ))),
                span: Span::DUMMY,
            }],
            return_ty: None,
            body: Some(body_id),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: Vec::new(),
            abi: None,
        }),
        visibility: Visibility::Public,
        span: Span::DUMMY,
    };

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    items.push(item);
    let mut body_owners = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));
    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = make_ty_ctx();
    let def_map = empty_def_map();
    let mut solver = MockSolver::new().respond_for_any(glyim_solve::SolverResult::Proven);
    let (ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    for d in &result.diagnostics {
        eprintln!("DIAG: {d:?}");
    }
    assert_no_errors(&result.diagnostics);

    let (_, thir_body) = &result.thir_bodies[0];
    let closures = collect_closures(thir_body);
    assert_eq!(closures.len(), 1, "exactly one closure should be produced");

    let (captures, closure_ty) = match &closures[0].kind {
        thir::ExprKind::Closure { captures, .. } => (captures.clone(), closures[0].ty),
        _ => panic!("expected a closure"),
    };
    assert_eq!(
        captures.len(),
        1,
        "move closure must capture exactly one variable (x)"
    );
    assert_eq!(
        captures[0].kind,
        thir::CaptureKind::ByValue,
        "move closure captures enclosing binding by value"
    );
    assert!(
        matches!(ctx.ty_kind(closure_ty), glyim_type::TyKind::Closure(_, _)),
        "closure type should resolve to a concrete Closure, got {closure_ty:?}"
    );
}
