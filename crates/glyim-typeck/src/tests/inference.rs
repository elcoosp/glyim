use super::test_utils::{empty_def_map, make_ty_ctx};
use crate::tests::test_utils::global_interner;
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::Visibility;
use glyim_core::primitives::*;
use glyim_hir::{Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind, Pat, PatId};
use glyim_span::Span;
use glyim_test::{assert_no_errors, mock::MockSolver};

#[test]
fn inference_param_type() {
    let inter = global_interner();
    let main_name = inter.intern("main");
    let x_name = inter.intern("x");

    let mut pats: IndexVec<PatId, Pat> = IndexVec::new();
    let x_pat = pats.push(Pat::Binding {
        name: x_name,
        mutability: Mutability::Not,
        subpattern: None,
    });

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    exprs.push(Expr::Path(glyim_hir::Path::from_single(x_name)));

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: vec![x_pat],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    };
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    let body_id = bodies.push(body);

    let param = glyim_hir::Param {
        name: x_name,
        ty: None, // no type annotation
        span: Span::DUMMY,
    };

    let item = Item {
        id: ItemId::from_raw(0),
        name: main_name,
        kind: ItemKind::Fn(FnItem {
            params: vec![param],
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
