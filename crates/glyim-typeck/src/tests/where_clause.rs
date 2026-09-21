use crate::tests::test_utils::global_interner;
use glyim_core::Visibility;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, LocalDefId};
use glyim_core::interner::Interner;
use glyim_def_map::*;
use glyim_hir::where_clause::*;
use glyim_hir::*;
use glyim_solve::{SolverIteratorNextInfo, SolverResult, TraitSolver};
use glyim_span::Span;
use glyim_test::{assert_has_errors, assert_no_errors};
use glyim_type::*;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn dummy_span() -> Span {
    Span::DUMMY
}

/// Build a CrateDefMap with the given names registered as types in the root scope.
fn build_def_map_with_names(
    interner: &mut Interner,
    krate: CrateId,
    names: &[&str],
) -> CrateDefMap {
    let mut scope = ItemScope::default();
    for (i, &name_str) in names.iter().enumerate() {
        let name = interner.intern(name_str);
        scope.types.insert(
            name,
            (
                LocalDefId::from_raw(i as u32),
                Visibility::Public,
                Span::DUMMY,
            ),
        );
    }
    let root_id = ModuleId::from_raw(0);
    let root_data = ModuleData {
        parent: None,
        children: vec![],
        scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };
    let mut modules = IndexVec::new();
    modules.push(root_data);
    CrateDefMap {
        root: root_id,
        modules,
        krate,
        interner: interner.clone(),
    variant_map: Default::default(),
    max_local_def_id: 0,
    }
}

/// Build a minimal CrateHir with one function item.
fn build_simple_hir(
    inter: &mut Interner,
    generic_params: Vec<GenericParam>,
    params: Vec<Param>,
    ret_ty: Option<TypeRef>,
    body_exprs: Vec<Expr>,
    where_clauses: Vec<WhereClause>,
) -> (CrateHir, BodyId) {
    let mut hir = CrateHir {
        items: IndexVec::new(),
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    };
    let mut exprs = IndexVec::new();
    for e in body_exprs {
        exprs.push(e);
    }
    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: Vec::new(),
        span: dummy_span(),
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.len()]),
    };
    let body_id = hir.bodies.push(body);

    let item = Item {
        id: ItemId::from_raw(0),
        name: inter.intern("test_fn"),
        kind: ItemKind::Fn(FnItem {
            params: params.clone(),
            return_ty: ret_ty,
            body: Some(body_id),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params,
            where_clauses,
        abi: None,}),
        visibility: Visibility::Public,
        span: dummy_span(),
    };
    hir.items.push(item);

    let mut owners = IndexVec::new();
    owners.push(LocalDefId::from_raw(0));
    hir.body_owners = owners;

    (hir, body_id)
}

struct ApproveSolver;

impl TraitSolver for ApproveSolver {
    fn can_prove(&mut self, _ctx: &TyCtx, _predicate: &TraitPredicate) -> SolverResult {
        SolverResult::Proven
    }

    fn evaluate_predicate(&mut self, _ctx: &TyCtx, predicate: &Predicate) -> SolverResult {
        match predicate {
            Predicate::Trait(tp) => self.can_prove(_ctx, tp),
            _ => SolverResult::Proven,
        }
    }

    fn iterator_next_info(
        &self,
        _ctx_mut: &mut TyCtxMut,
        _iter_ty: Ty,
        _elem_ty: Ty,
    ) -> Option<SolverIteratorNextInfo> {
        None
    }
}

struct RejectSolver;

impl TraitSolver for RejectSolver {
    fn can_prove(&mut self, _ctx: &TyCtx, _predicate: &TraitPredicate) -> SolverResult {
        SolverResult::DefiniteNo
    }

    fn evaluate_predicate(&mut self, _ctx: &TyCtx, predicate: &Predicate) -> SolverResult {
        match predicate {
            Predicate::Trait(tp) => self.can_prove(_ctx, tp),
            _ => SolverResult::DefiniteNo,
        }
    }

    fn iterator_next_info(
        &self,
        _ctx_mut: &mut TyCtxMut,
        _iter_ty: Ty,
        _elem_ty: Ty,
    ) -> Option<SolverIteratorNextInfo> {
        None
    }
}
// Helper to create a generic type param `T` at index 0
fn ty_param(inter: &mut Interner, name: &str) -> GenericParam {
    GenericParam {
        name: inter.intern(name),
        kind: GenericParamKind::Type { default: None, bounds: Vec::new() },
        span: dummy_span(),
    }
}

// ---------------------------------------------------------------------------
// V02-T01: Function with `where T: Clone` → compiles if T implements Clone
// ---------------------------------------------------------------------------
#[test]
fn t01_fn_where_clone_satisfied() {
    let mut inter = global_interner();
    let name_clone = inter.intern("Clone");
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![TraitBound {
            trait_path: Path::from_single(name_clone),
            span: dummy_span(),
            fn_shape: None,
        }],
        span: dummy_span(),
    };

    let (hir, _body_id) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = ApproveSolver;
    let def_map = build_def_map_with_names(&mut inter, CrateId::from_raw(0), &["Clone", "T"]);
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}

// ---------------------------------------------------------------------------
// V02-T02: Trait with supertrait; impl must satisfy both → compiles
// ---------------------------------------------------------------------------
#[test]
fn t02_supertrait_impl_satisfies_both() {
    let mut inter = global_interner();
    let name_clone = inter.intern("Clone");
    let name_copy = inter.intern("Copy");
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![
            TraitBound {
                trait_path: Path::from_single(name_copy),
                span: dummy_span(),
            fn_shape: None,
        },
            TraitBound {
                trait_path: Path::from_single(name_clone),
                span: dummy_span(),
            fn_shape: None,
        },
        ],
        span: dummy_span(),
    };

    let (hir, _) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = ApproveSolver;
    let def_map =
        build_def_map_with_names(&mut inter, CrateId::from_raw(0), &["Copy", "Clone", "T"]);
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}

// ---------------------------------------------------------------------------
// V02-T04: Multiple where bounds
// ---------------------------------------------------------------------------
#[test]
fn t04_multiple_where_bounds() {
    let mut inter = global_interner();
    let name_clone = inter.intern("Clone");
    let name_debug = inter.intern("Debug");
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![
            TraitBound {
                trait_path: Path::from_single(name_clone),
                span: dummy_span(),
            fn_shape: None,
        },
            TraitBound {
                trait_path: Path::from_single(name_debug),
                span: dummy_span(),
            fn_shape: None,
        },
        ],
        span: dummy_span(),
    };

    let (hir, _) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = ApproveSolver;
    let def_map =
        build_def_map_with_names(&mut inter, CrateId::from_raw(0), &["Clone", "Debug", "T"]);
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}

// ---------------------------------------------------------------------------
// V02-T06: Missing supertrait impl → error
// ---------------------------------------------------------------------------
#[test]
fn t06_missing_supertrait_error() {
    let mut inter = global_interner();
    let name_clone = inter.intern("Clone");
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![TraitBound {
            trait_path: Path::from_single(name_clone),
            span: dummy_span(),
            fn_shape: None,
        }],
        span: dummy_span(),
    };

    let (hir, _) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = RejectSolver;
    let def_map = build_def_map_with_names(&mut inter, CrateId::from_raw(0), &["Clone", "T"]);
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_has_errors(&result.diagnostics);
}

// ---------------------------------------------------------------------------
// V02-T07: Where bound not satisfied → error
// ---------------------------------------------------------------------------
#[test]
fn t07_where_bound_not_satisfied_error() {
    let mut inter = global_interner();
    let name_copy = inter.intern("Copy");
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![TraitBound {
            trait_path: Path::from_single(name_copy),
            span: dummy_span(),
            fn_shape: None,
        }],
        span: dummy_span(),
    };

    let (hir, _) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = RejectSolver;
    let def_map = build_def_map_with_names(&mut inter, CrateId::from_raw(0), &["Copy", "T"]);
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_has_errors(&result.diagnostics);
}

/// Build a CrateDefMap where `mod_name::trait_name` is resolvable: a child
/// module whose scope contains the trait name mapped to `trait_local_id`.
fn build_def_map_with_nested_trait(
    interner: &mut Interner,
    krate: CrateId,
    mod_name: &str,
    trait_name: &str,
    trait_local_id: u32,
) -> CrateDefMap {
    let mod_n = interner.intern(mod_name);
    let trait_n = interner.intern(trait_name);

    let mut root_scope = ItemScope::default();
    let root_id = ModuleId::from_raw(0);
    let child_id = ModuleId::from_raw(1);
    root_scope.types.insert(
        mod_n,
        (LocalDefId::from_raw(0), Visibility::Public, Span::DUMMY),
    );
    let root_data = ModuleData {
        parent: None,
        children: vec![(mod_n, child_id)],
        scope: root_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };

    let mut child_scope = ItemScope::default();
    child_scope.types.insert(
        trait_n,
        (LocalDefId::from_raw(trait_local_id), Visibility::Public, Span::DUMMY),
    );
    let child_data = ModuleData {
        parent: Some(root_id),
        children: vec![],
        scope: child_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(1),
        visibility: Visibility::Public,
    };

    let mut modules = IndexVec::new();
    modules.push(root_data);
    modules.push(child_data);
    CrateDefMap {
        root: root_id,
        modules,
        krate,
        interner: interner.clone(),
    variant_map: Default::default(),
    max_local_def_id: 0,
    }
}

// ---------------------------------------------------------------------------
// V02-T08: Multi-segment trait path in a where clause (`inner::Clone2`) must
// resolve through the module tree (not bail with "not yet supported").
// ---------------------------------------------------------------------------
#[test]
fn t08_where_clause_multi_segment_path_resolves() {
    let mut inter = global_interner();
    let mod_name = "inner";
    let trait_name = "Clone2";
    let trait_local_id = 7u32;
    let name_t = inter.intern("T");

    let generic_params = vec![ty_param(&mut inter, "T")];
    let t_ty = TypeRef::Path(Path::from_single(name_t));
    let param = Param {
        name: name_t,
        ty: Some(t_ty.clone()),
        span: dummy_span(),
    };
    let body_exprs = vec![Expr::Literal(Literal::Unit)];

    // Build the multi-segment path `inner::Clone2`.
    let mut trait_path = Path::from_single(inter.intern(trait_name));
    trait_path.segments.insert(
        0,
        glyim_hir::PathSegment {
            name: inter.intern(mod_name),
            generic_args: None,
        },
    );
    let wc = WhereClause {
        ty: t_ty.clone(),
        bounds: vec![TraitBound {
            trait_path,
            span: dummy_span(),
            fn_shape: None,
        }],
        span: dummy_span(),
    };

    let (hir, _) = build_simple_hir(
        &mut inter,
        generic_params,
        vec![param],
        None,
        body_exprs,
        vec![wc],
    );

    let ctx = TyCtxMut::new(inter.clone());
    let mut solver = ApproveSolver;
    let def_map = build_def_map_with_nested_trait(
        &mut inter,
        CrateId::from_raw(0),
        mod_name,
        trait_name,
        trait_local_id,
    );
    let (_tcx, result) = crate::typeck_crate(ctx, &def_map, &hir, &mut solver);
    assert_no_errors(&result.diagnostics);
}
