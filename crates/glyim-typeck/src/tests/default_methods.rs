/// Tests for V03: Default Methods
use crate::tests::test_utils::global_interner;
use crate::typeck_crate;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, LocalDefId};
use glyim_core::interner::Interner;
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
use glyim_hir::*;
use glyim_solve::SolverResult;
use glyim_span::Span;
use glyim_test::mock::MockSolver;
use glyim_type::*;

fn make_interner() -> Interner {
    global_interner()
}

fn make_name(interner: &mut Interner, s: &str) -> Name {
    interner.intern(s)
}

fn make_return_42_body(owner: LocalDefId) -> Body {
    let mut exprs = IndexVec::new();
    let lit_expr = Expr::Literal(Literal::Int(42, None));
    let lit_id = exprs.push(lit_expr);
    let ret_expr = Expr::Return {
        value: Some(lit_id),
    };
    let _ret_id = exprs.push(ret_expr);
    Body {
        owner,
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    }
}

fn make_return_99_body(owner: LocalDefId) -> Body {
    let mut exprs = IndexVec::new();
    let lit_expr = Expr::Literal(Literal::Int(99, None));
    let lit_id = exprs.push(lit_expr);
    let ret_expr = Expr::Return {
        value: Some(lit_id),
    };
    let _ret_id = exprs.push(ret_expr);
    Body {
        owner,
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    }
}

fn make_body_with_call(owner: LocalDefId, callee_name: Name) -> Body {
    let mut exprs = IndexVec::new();
    let path = Path::from_single(callee_name);
    let callee_path_id = exprs.push(Expr::Path(path));
    let call_id = exprs.push(Expr::Call {
        func: callee_path_id,
        args: vec![],
    });
    let _ret_id = exprs.push(Expr::Return {
        value: Some(call_id),
    });
    Body {
        owner,
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.clone().len()]),
    }
}

fn build_empty_def_map(krate: CrateId) -> CrateDefMap {
    let mut modules = IndexVec::new();
    let root = ModuleId::from_raw(0);
    modules.push(ModuleData {
        parent: None,
        children: Vec::new(),
        scope: ItemScope::default(),
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    });
    CrateDefMap {
        root,
        modules,
        krate,
        interner: global_interner(),
    variant_map: Default::default(),
    }
}

fn make_simple_hir_with_trait_and_impl(override_default: bool) -> (TyCtxMut, CrateHir) {
    let mut interner = make_interner();
    let trait_name = make_name(&mut interner, "MyTrait");
    let method_name = make_name(&mut interner, "my_method");

    let owner_trait = LocalDefId::from_raw(0);
    let owner_impl = LocalDefId::from_raw(1);

    let default_body = make_return_42_body(owner_trait);

    let trait_method = TraitMethod {
        name: method_name,
        params: vec![],
        return_ty: None,
        default_body: None,
    };

    let trait_item = TraitItem {
        associated_types: vec![],
        methods: vec![trait_method],
        generic_params: vec![],
        where_clauses: vec![],
    };

    let (impl_body_opt, override_body) = if override_default {
        let body = make_return_99_body(owner_impl);
        (Some(BodyId::from_raw(1)), Some(body))
    } else {
        (None, None)
    };

    let impl_method = ImplMethod {
        name: method_name,
        body: impl_body_opt,
        params: vec![],
        generic_params: vec![],
        return_ty: None,
    };

    let impl_item = ImplItem {
        trait_ref: Some(Path {
            segments: vec![PathSegment {
                name: trait_name,
                generic_args: None,
            }],
            kind: glyim_core::path::PathKind::Plain,
        }),
        self_ty: TypeRef::Path(Path::from_single(make_name(&mut interner, "MyType"))),
        methods: vec![impl_method],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    };

    let mut items = IndexVec::new();
    let trait_item_id = items.push(Item {
        id: ItemId::from_raw(0),
        name: trait_name,
        kind: ItemKind::Trait(trait_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });
    if let ItemKind::Trait(ref mut ti) = items[trait_item_id].kind {
        ti.methods[0].default_body = Some(BodyId::from_raw(0));
    }

    items.push(Item {
        id: ItemId::from_raw(1),
        name: make_name(&mut interner, "MyType"),
        kind: ItemKind::Impl(impl_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });

    let mut bodies = IndexVec::new();
    bodies.push(default_body);
    let mut body_owners = IndexVec::new();
    body_owners.push(owner_trait);
    if let Some(override_body) = override_body {
        bodies.push(override_body);
        body_owners.push(owner_impl);
    }

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = TyCtxMut::new(interner);
    (ctx, hir)
}

#[test]
fn v03_t01_trait_default_method_impl_no_override() {
    let (ctx, hir) = make_simple_hir_with_trait_and_impl(false);
    let def_map = build_empty_def_map(CrateId::from_raw(0));
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    assert!(
        !result.thir_bodies.is_empty(),
        "Expected at least one THIR body"
    );
    let (_owner, thir_body) = &result.thir_bodies[0];
    assert_eq!(thir_body.stmts.len(), 2);
    assert!(matches!(
        thir_body.stmts[1],
        crate::thir::Stmt::Return { .. }
    ));
}

#[test]
fn v03_t02_overridden_default_method() {
    let (ctx, hir) = make_simple_hir_with_trait_and_impl(true);
    let def_map = build_empty_def_map(CrateId::from_raw(0));
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    assert!(!result.thir_bodies.is_empty());
    let (_owner, thir_body) = &result.thir_bodies[0];
    assert_eq!(thir_body.stmts.len(), 2);
    assert!(matches!(
        thir_body.stmts[1],
        crate::thir::Stmt::Return { .. }
    ));
}

#[test]
fn v03_t03_default_method_calling_another_default_method() {
    let mut interner = make_interner();
    let trait_name = make_name(&mut interner, "MyTrait");
    let bar_name = make_name(&mut interner, "bar");
    let foo_name = make_name(&mut interner, "foo");

    let owner_trait = LocalDefId::from_raw(0);
    let _owner_impl = LocalDefId::from_raw(1);

    let bar_body = make_return_42_body(owner_trait);
    let foo_body = make_body_with_call(owner_trait, bar_name);

    let trait_methods = vec![
        TraitMethod {
            name: bar_name,
            params: vec![],
            return_ty: None,
            default_body: None,
        },
        TraitMethod {
            name: foo_name,
            params: vec![],
            return_ty: None,
            default_body: None,
        },
    ];

    let trait_item = TraitItem {
        associated_types: vec![],
        methods: trait_methods,
        generic_params: vec![],
        where_clauses: vec![],
    };

    let impl_methods = vec![
        ImplMethod {
            name: bar_name,
            body: None,
            params: vec![],
            generic_params: vec![],
            return_ty: None,
        },
        ImplMethod {
            name: foo_name,
            body: None,
            params: vec![],
            generic_params: vec![],
            return_ty: None,
        },
    ];

    let impl_item = ImplItem {
        trait_ref: Some(Path {
            segments: vec![PathSegment {
                name: trait_name,
                generic_args: None,
            }],
            kind: glyim_core::path::PathKind::Plain,
        }),
        self_ty: TypeRef::Path(Path::from_single(make_name(&mut interner, "MyType"))),
        methods: impl_methods,
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    };

    let mut items = IndexVec::new();
    let trait_item_id = items.push(Item {
        id: ItemId::from_raw(0),
        name: trait_name,
        kind: ItemKind::Trait(trait_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });
    if let ItemKind::Trait(ref mut ti) = items[trait_item_id].kind {
        ti.methods[0].default_body = Some(BodyId::from_raw(0));
        ti.methods[1].default_body = Some(BodyId::from_raw(1));
    }

    items.push(Item {
        id: ItemId::from_raw(1),
        name: make_name(&mut interner, "MyType"),
        kind: ItemKind::Impl(impl_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });

    let mut bodies = IndexVec::new();
    bodies.push(bar_body);
    bodies.push(foo_body);
    let body_owners = IndexVec::from_raw(vec![owner_trait, owner_trait]);

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = TyCtxMut::new(interner);
    let def_map = build_empty_def_map(CrateId::from_raw(0));
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    assert_eq!(result.thir_bodies.len(), 2, "Expected two method bodies");
    let has_call = result.thir_bodies.iter().any(|(_owner, body)| {
        body.stmts.iter().any(|stmt| {
            if let crate::thir::Stmt::Expr { expr } = stmt {
                matches!(expr.kind, crate::thir::ExprKind::Call { .. })
            } else {
                false
            }
        })
    });
    assert!(has_call, "Expected a Call expression in one of the bodies");
}

#[test]
fn v03_t04_default_method_with_generic_params() {
    let mut interner = make_interner();
    let trait_name = make_name(&mut interner, "MyTrait");
    let method_name = make_name(&mut interner, "my_method");
    let t_name = make_name(&mut interner, "T");

    let owner_trait = LocalDefId::from_raw(0);

    // Use a unit body to avoid type mismatches
    let mut exprs = IndexVec::new();
    exprs.push(Expr::Literal(Literal::Unit));
    let default_body = Body {
        owner: owner_trait,
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: vec![],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.len()]),
    };

    let generic_params = vec![GenericParam {
        name: t_name,
        kind: GenericParamKind::Type { default: None, bounds: Vec::new() },
        span: Span::DUMMY,
    }];

    let trait_method = TraitMethod {
        name: method_name,
        params: vec![],
        return_ty: None,
        default_body: None,
    };

    let trait_item = TraitItem {
        associated_types: vec![],
        methods: vec![trait_method],
        generic_params: generic_params.clone(),
        where_clauses: vec![],
    };

    let impl_method = ImplMethod {
        name: method_name,
        body: None,
        params: vec![],
        generic_params: vec![],
        return_ty: None,
    };

    let impl_item = ImplItem {
        trait_ref: Some(Path {
            segments: vec![PathSegment {
                name: trait_name,
                generic_args: Some(vec![TypeRef::Path(Path::from_single(make_name(
                    &mut interner,
                    "i32",
                )))]),
            }],
            kind: glyim_core::path::PathKind::Plain,
        }),
        self_ty: TypeRef::Path(Path::from_single(make_name(&mut interner, "MyType"))),
        methods: vec![impl_method],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    };

    let mut items = IndexVec::new();
    let trait_item_id = items.push(Item {
        id: ItemId::from_raw(0),
        name: trait_name,
        kind: ItemKind::Trait(trait_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });
    if let ItemKind::Trait(ref mut ti) = items[trait_item_id].kind {
        ti.methods[0].default_body = Some(BodyId::from_raw(0));
    }

    items.push(Item {
        id: ItemId::from_raw(1),
        name: make_name(&mut interner, "MyType"),
        kind: ItemKind::Impl(impl_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });

    let mut bodies = IndexVec::new();
    bodies.push(default_body);
    let body_owners = IndexVec::from_raw(vec![owner_trait]);

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = TyCtxMut::new(interner.clone());

    // Build def map with MyType and MyTrait so they resolve
    let mut scope = glyim_def_map::ItemScope::default();
    scope.types.insert(
        make_name(&mut interner, "MyType"),
        (LocalDefId::from_raw(0), Visibility::Public, Span::DUMMY),
    );
    scope.types.insert(
        make_name(&mut interner, "MyTrait"),
        (LocalDefId::from_raw(1), Visibility::Public, Span::DUMMY),
    );
    let root_id = glyim_def_map::ModuleId::from_raw(0);
    let root_data = glyim_def_map::ModuleData {
        parent: None,
        children: vec![],
        scope,
        origin: glyim_def_map::ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };
    let mut modules = IndexVec::new();
    modules.push(root_data);
    let def_map = glyim_def_map::CrateDefMap {
        root: root_id,
        modules,
        krate: CrateId::from_raw(0),
        interner: interner.clone(),
    variant_map: Default::default(),
    };

    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    assert!(!result.thir_bodies.is_empty());
}
#[test]
fn v03_t05_default_method_calls_missing_method_error() {
    let mut interner = make_interner();
    let trait_name = make_name(&mut interner, "MyTrait");
    let method_name = make_name(&mut interner, "missing");

    let trait_method = TraitMethod {
        name: method_name,
        params: vec![],
        return_ty: None,
        default_body: None,
    };

    let trait_item = TraitItem {
        associated_types: vec![],
        methods: vec![trait_method],
        generic_params: vec![],
        where_clauses: vec![],
    };

    let impl_method = ImplMethod {
        name: method_name,
        body: None,
        params: vec![],
        generic_params: vec![],
        return_ty: None,
    };

    let impl_item = ImplItem {
        trait_ref: Some(Path {
            segments: vec![PathSegment {
                name: trait_name,
                generic_args: None,
            }],
            kind: glyim_core::path::PathKind::Plain,
        }),
        self_ty: TypeRef::Path(Path::from_single(make_name(&mut interner, "MyType"))),
        methods: vec![impl_method],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    };

    let mut items = IndexVec::new();
    items.push(Item {
        id: ItemId::from_raw(0),
        name: trait_name,
        kind: ItemKind::Trait(trait_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });
    items.push(Item {
        id: ItemId::from_raw(1),
        name: make_name(&mut interner, "MyType"),
        kind: ItemKind::Impl(impl_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    });

    let hir = CrateHir {
        items,
        bodies: IndexVec::new(),
        body_owners: IndexVec::new(),
        interner: Default::default(),
    };

    let ctx = TyCtxMut::new(interner);
    let def_map = build_empty_def_map(CrateId::from_raw(0));
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    assert!(
        !result.diagnostics.is_empty(),
        "Expected an error diagnostic"
    );
    let error_msg = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("has no implementation and no default"));
    assert!(
        error_msg,
        "Expected missing method diagnostic, got: {:?}",
        result.diagnostics
    );
}

/// §9.2: when the same method is provided by two distinct `impl` blocks for
/// the same self type, a call to that method is ambiguous (rustc's E0034).
/// The type checker must surface *all* candidates rather than silently
/// resolving to the first matching impl.
#[test]
fn v03_t06_ambiguous_method_in_multiple_impls() {
    let mut interner = make_interner();
    let method_name = make_name(&mut interner, "amb");
    let mytype_name = make_name(&mut interner, "i32");

    // Two inherent impls of `i32`, each providing `amb`. Using a primitive
    // self type guarantees both impls resolve and unify with the receiver.
    let mk_impl = |owner: LocalDefId| Item {
        id: ItemId::from_raw(owner.to_raw()),
        name: mytype_name,
        kind: ItemKind::Impl(ImplItem {
            trait_ref: None,
            self_ty: TypeRef::Path(Path::from_single(mytype_name)),
            methods: vec![ImplMethod {
                name: method_name,
                body: None,
                params: vec![],
                return_ty: None,
            }],
            generic_params: vec![],
            where_clauses: vec![],
            associated_types: vec![],
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    let mut items = IndexVec::new();
    items.push(mk_impl(LocalDefId::from_raw(1)));
    items.push(mk_impl(LocalDefId::from_raw(2)));

    // fn main(x: MyType) { x.amb(); }
    let mut pats: IndexVec<PatId, Pat> = IndexVec::new();
    let x_pat = pats.push(Pat::Binding {
        name: make_name(&mut interner, "x"),
        mutability: Mutability::Not,
        subpattern: None,
    });
    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let x_ref = exprs.push(Expr::Path(Path::from_single(make_name(&mut interner, "x"))));
    let method_call = exprs.push(Expr::MethodCall {
        receiver: x_ref,
        method: method_name,
        args: vec![],
    });
    exprs.push(Expr::Return {
        value: Some(method_call),
    });

    let main_body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: vec![x_pat],
        span: Span::DUMMY,
        expr_spans: IndexVec::from_raw(vec![Span::DUMMY; exprs.len()]),
    };
    let mut bodies = IndexVec::new();
    let main_body_id = bodies.push(main_body);
    let mut body_owners = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));

    items.push(Item {
        id: ItemId::from_raw(0),
        name: make_name(&mut interner, "main"),
        kind: ItemKind::Fn(FnItem {
            params: vec![glyim_hir::Param {
                name: make_name(&mut interner, "x"),
                ty: Some(TypeRef::Path(Path::from_single(mytype_name))),
                span: Span::DUMMY,
            }],
            return_ty: None,
            body: Some(main_body_id),
            is_unsafe: false,
            is_async: false,
            is_const: false,
            generic_params: vec![],
            where_clauses: vec![],
        abi: None,}),
        visibility: Visibility::Public,
        span: Span::DUMMY,
    });

    let hir = CrateHir {
        items,
        bodies,
        body_owners,
        interner: Default::default(),
    };

    let ctx = TyCtxMut::new(interner);
    let def_map = build_empty_def_map(CrateId::from_raw(0));
    let mut solver = MockSolver::new().respond_for_any(SolverResult::Proven);
    let (_frozen_ctx, result) = typeck_crate(ctx, &def_map, &hir, &mut solver);

    let ambiguous = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("ambiguous method"));
    assert!(
        ambiguous,
        "Expected an ambiguous-method diagnostic, got: {:?}",
        result.diagnostics
    );
}
