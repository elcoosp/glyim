//! Value-namespace path resolution for expressions (plan: value paths).
//!
//! `check_path` now resolves value paths (functions, via `FnRef`/`TyKind::FnDef`)
//! through the def map's value namespace, for both single- and multi-segment
//! paths (`foo`, `mod::foo`). This test verifies that a function value path
//! resolves to a `FnRef` THIR node carrying a `TyKind::FnDef` type, and that
//! calling it yields the registered return type with no diagnostics.
use super::common::*;
use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::Visibility;
use glyim_def_map::ItemScope;
use glyim_hir::*;
use glyim_span::Span;
use glyim_type::{FnSig, Substitution, Ty, TyCtxMut, TyKind};

fn i32_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32))
}

/// Build a def map whose root *value* namespace maps `foo` to LocalDefId(5).
fn def_map_with_fn_value(nm: Name) -> glyim_def_map::CrateDefMap {
    let mut dm = empty_def_map();
    let root = &mut dm.modules[dm.root];
    let mut scope = ItemScope::default();
    scope.values.insert(
        nm,
        (LocalDefId::from_raw(5), Visibility::Public, Span::DUMMY),
    );
    root.scope = scope;
    dm
}

fn register_fn(ctx: &mut TyCtxMut, def_id: FnDefId) {
    let sig = FnSig {
        inputs: Substitution::empty(),
        output: i32_ty(ctx),
        c_variadic: false,
        unsafety: glyim_core::primitives::Safety::Safe,
        abi: glyim_core::primitives::Abi::Glyim,
    };
    ctx.register_fn_sig(def_id, sig);
}

fn owner_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

#[test]
fn function_value_path_resolves_to_fnref() {
    let foo = name("foo");
    let def_map = def_map_with_fn_value(foo);
    let mut ctx = make_ty_ctx();
    let fn_def_id = FnDefId::from_raw(5);
    register_fn(&mut ctx, fn_def_id);

    // Body: a single expression `foo` (a function reference).
    let mut exprs = Vec::new();
    exprs.push(Expr::Path(Path::from_single(foo)));
    let (hir, body_id) = make_single_body_hir(exprs);

    let mut infer = glyim_solve::InferenceTable::new();
    let (thir_body, diags) = check_function_body(
        &mut ctx,
        &mut infer,
        &def_map,
        &hir,
        body_id,
        owner_def_id(),
        Ty::UNIT,
        &[],
    );

    assert!(
        diags.is_empty(),
        "function value path should resolve with no diagnostics, got: {:?}",
        diags
    );

    let stmt = &thir_body.stmts[0];
    let expr = match stmt {
        crate::thir::Stmt::Expr { expr, .. } => expr,
        other => panic!("expected Expr statement, got {:?}", other),
    };
    match &expr.kind {
        crate::thir::ExprKind::FnRef(id) => assert_eq!(*id, fn_def_id),
        other => panic!("expected FnRef, got {:?}", other),
    }
    assert!(
        matches!(ctx.ty_kind(expr.ty), TyKind::FnDef(id, _) if *id == fn_def_id),
        "function reference must have a TyKind::FnDef type"
    );
}

#[test]
fn calling_function_value_path_yields_return_type() {
    let foo = name("foo");
    let def_map = def_map_with_fn_value(foo);
    let mut ctx = make_ty_ctx();
    let fn_def_id = FnDefId::from_raw(5);
    register_fn(&mut ctx, fn_def_id);

    // Body: `foo()` — call the function reference.
    let mut exprs = Vec::new();
    exprs.push(Expr::Path(Path::from_single(foo)));
    exprs.push(Expr::Call {
        func: ExprId::from_raw(0),
        args: vec![],
    });
    let (hir, body_id) = make_single_body_hir(exprs);

    let mut infer = glyim_solve::InferenceTable::new();
    let (thir_body, diags) = check_function_body(
        &mut ctx,
        &mut infer,
        &def_map,
        &hir,
        body_id,
        owner_def_id(),
        Ty::UNIT,
        &[],
    );

    assert!(
        diags.is_empty(),
        "calling a resolved function should produce no diagnostics, got: {:?}",
        diags
    );

    let call_stmt = &thir_body.stmts[1];
    let ty = match call_stmt {
        crate::thir::Stmt::Expr { expr, .. } => expr.ty,
        other => panic!("expected Expr statement, got {:?}", other),
    };
    assert_eq!(
        *ctx.ty_kind(ty),
        TyKind::Int(glyim_core::primitives::IntTy::I32),
        "call to foo() must yield its registered return type i32"
    );
}
