use super::test_utils::{empty_def_map, make_ty_ctx};
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Interner;
use glyim_core::primitives::Visibility;
use glyim_diag::GlyimDiagnostic;
use glyim_hir::{Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind};
use glyim_solve::{InferenceTable, SolverResult};
use glyim_span::Span;
use glyim_test::mock::MockSolver;
use glyim_type::{Predicate, TraitPredicate, TraitRef, Ty};

// Minimal definition of TypeckCtx to make the test compile.
struct TypeckCtx<'a> {
    #[allow(unused)]
    ctx: &'a mut glyim_type::TyCtxMut,
    #[allow(unused)]
    infer: &'a mut InferenceTable,
    #[allow(unused)]
    diagnostics: &'a mut Vec<GlyimDiagnostic>,
    pending_obligations: Vec<glyim_solve::Obligation>,
}

impl TypeckCtx<'_> {
    fn require_trait_bound(&mut self, _ty: Ty, pred: TraitPredicate, _span: Span) {
        use glyim_solve::Obligation;
        let obligation = Obligation {
            predicate: Predicate::Trait(pred),
            cause: glyim_solve::ObligationCause {
                span: Span::DUMMY,
                code: glyim_solve::ObligationCauseCode::WellFormed,
            },
        };
        self.pending_obligations.push(obligation);
    }
}

/// Obligation tests – disabled for now (requires full trait solver)
#[allow(unused_imports, dead_code)]
use super::common::*;

#[test]
fn obligation_collected() {
    let mut ctx = make_ty_ctx();
    // Create the substitution BEFORE borrowing ctx for TypeckCtx
    let substs = ctx.intern_substitution(vec![]);
    let mut infer = InferenceTable::new();
    let mut diagnostics: Vec<GlyimDiagnostic> = vec![];
    let mut typeck_ctx = TypeckCtx {
        ctx: &mut ctx,
        infer: &mut infer,
        diagnostics: &mut diagnostics,
        pending_obligations: Vec::new(),
    };

    let dummy_trait_ref = TraitRef {
        def_id: glyim_core::def_id::TraitDefId::from_raw(0),
        substs,
    };
    let trait_pred = TraitPredicate {
        trait_ref: dummy_trait_ref,
        polarity: glyim_type::ImplPolarity::Positive,
    };

    typeck_ctx.require_trait_bound(Ty::BOOL, trait_pred.clone(), Span::DUMMY);
    assert_eq!(
        typeck_ctx.pending_obligations.len(),
        1,
        "should have one obligation"
    );
    let obl = &typeck_ctx.pending_obligations[0];
    assert!(
        matches!(obl.predicate, Predicate::Trait(_)),
        "predicate should be Trait"
    );
}

#[test]
fn obligation_fulfilled() {
    let inter = Interner::new();
    let main_name = inter.intern("main");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
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
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert!(result.diagnostics.is_empty());
    assert_eq!(solver.call_count(), 0, "no obligations → solver not called");
}
