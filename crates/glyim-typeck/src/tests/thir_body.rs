use super::test_utils::{empty_def_map, make_ty_ctx};
use crate::tests::test_utils::global_interner;
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::primitives::*;
use glyim_hir::{Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind};
use glyim_span::Span;
use glyim_test::{assert_no_errors, mock::MockSolver};

#[test]
fn thir_body_constructed() {
    let inter = global_interner();
    let main_name = inter.intern("main");
    let x_name = inter.intern("x");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    exprs.push(Expr::Path(glyim_hir::Path::from_single(x_name)));
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
            params: vec![glyim_hir::Param {
                name: x_name,
                ty: Some(glyim_hir::TypeRef::Path(glyim_hir::Path::from_single(
                    inter.intern("i32"),
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
    let (_, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
    assert_eq!(
        result.thir_bodies.len(),
        1,
        "one THIR body should be produced"
    );
    let (_, thir_body) = &result.thir_bodies[0];
    assert_eq!(thir_body.params.len(), 1, "one parameter");
    assert_eq!(thir_body.params[0].name, x_name, "parameter name");
    assert!(
        thir_body.return_ty == glyim_type::Ty::UNIT,
        "return type should be unit"
    );
}
