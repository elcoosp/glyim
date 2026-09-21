use super::test_utils::{empty_def_map, make_ty_ctx};
use crate::tests::test_utils::global_interner;
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::Visibility;
use glyim_core::primitives::*;
use glyim_hir::{Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind};
use glyim_span::Span;
use glyim_test::{assert_no_errors, mock::MockSolver};

#[test]
fn binary_i32_add_ok() {
    let inter = global_interner();
    let main_name = inter.intern("main");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let lhs = exprs.push(Expr::Literal(glyim_hir::Literal::Int(1, Some(IntTy::I32))));
    let rhs = exprs.push(Expr::Literal(glyim_hir::Literal::Int(2, Some(IntTy::I32))));
    exprs.push(Expr::Binary {
        op: BinOp::Add,
        lhs,
        rhs,
    });
    exprs.push(Expr::Literal(glyim_hir::Literal::Unit));

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    };
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    let body_id = bodies.push(body);

    let item = Item {
        id: ItemId::from_raw(0),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![],
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
    let (_, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}

/// Tier 6.4: `TypeckResult::expr_ty` must return the real, resolved type of a
/// checked expression (not `None`, and not the old `#[cfg(test)]`-only
/// hardcoded `I32`). The binary-add expression (ExprId 2 in this fixture)
/// type-checks as `i32`.
#[test]
fn expr_ty_returns_resolved_type() {
    let inter = global_interner();
    let main_name = inter.intern("main");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let lhs = exprs.push(Expr::Literal(glyim_hir::Literal::Int(1, Some(IntTy::I32))));
    let rhs = exprs.push(Expr::Literal(glyim_hir::Literal::Int(2, Some(IntTy::I32))));
    let binary = exprs.push(Expr::Binary {
        op: BinOp::Add,
        lhs,
        rhs,
    });
    exprs.push(Expr::Literal(glyim_hir::Literal::Unit));

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    };
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    let body_id = bodies.push(body);

    let item = Item {
        id: ItemId::from_raw(0),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![],
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
    assert_no_errors(&result.diagnostics);

    let owner = LocalDefId::from_raw(0);
    let ty = result
        .expr_ty(owner, binary.to_raw() as usize)
        .expect("expr_ty should return a resolved type for the binary-add expression");
    // `expr_ty` returns a fully-resolved `Ty`. Inspect its kind via the frozen
    // `TyCtx` (returned alongside the result) — the binary-add of two i32
    // literals must resolve to `Int(I32)`, not an inference variable or ERROR.
    let kind = ctx.ty_kind(ty).clone();
    assert_eq!(
        kind,
        glyim_type::TyKind::Int(IntTy::I32),
        "binary-add of two i32 literals should resolve to i32"
    );
    assert_ne!(ty, glyim_type::Ty::ERROR);
}
