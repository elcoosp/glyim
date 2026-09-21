//! Value-namespace multi-segment path resolution through the real `check_path`.
//!
//! Builds a hand-crafted `CrateDefMap` whose root has a child module `math`
//! that declares a function `square` in its value namespace, registers the
//! function signature, and asserts that the path `math::square` (written as a
//! HIR expression `Expr::Path(math::square)`) resolves to
//! `thir::ExprKind::FnRef(math_square)` with type `TyKind::FnDef(math_square, [])`,
//! and that *calling* it (`math::square()`) yields the registered return type.
use super::common::*;
use glyim_core::primitives::{Abi, Safety, Visibility};
use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_core::interner::Name;
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleOrigin, Namespace};
use glyim_hir::*;
use glyim_span::Span;
use glyim_type::{FnSig, Substitution, Ty, TyCtxMut, TyKind};

fn owner_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn intern(s: &str) -> Name {
    global_interner().intern(s)
}

/// A def map with `root` containing child module `math`, and `math` declaring
/// `square: FnDef` in its value namespace.
fn def_map_with_module_fn() -> CrateDefMap {
    let math_name = intern("math");
    let square_name = intern("square");

    let mut modules = glyim_core::arena::IndexVec::new();
    // root module (ModuleId 0)
    let root = modules.push(ModuleData {
        parent: None,
        children: Vec::new(),
        scope: ItemScope::default(),
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    });
    // math module (ModuleId 1)
    let math_mod = modules.push(ModuleData {
        parent: Some(root),
        children: Vec::new(),
        scope: ItemScope::default(),
        origin: ModuleOrigin::Inline { span: Span::DUMMY },
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(1),
        visibility: Visibility::Public,
    });
    // root.child[math] = math_mod
    modules[root].children.push((math_name, math_mod));
    // math declares `square` as a value (FnDef).
    let square_def = LocalDefId::from_raw(2);
    modules[math_mod].scope.declare(
        square_name,
        square_def,
        Visibility::Public,
        Span::DUMMY,
        Namespace::Values,
    );

    CrateDefMap {
        root,
        modules,
        krate: CrateId::from_raw(0),
        interner: global_interner(),
    variant_map: Default::default(),
    max_local_def_id: 0,
    }
}

fn i32_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32))
}

fn register_square(ctx: &mut TyCtxMut) -> FnDefId {
    let square = FnDefId::from_raw(2);
    let sig = FnSig {
        inputs: Substitution::empty(),
        output: i32_ty(ctx),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx.register_fn_sig(square, sig);
    square
}

#[test]
fn multi_segment_function_path_resolves_to_fn_ref() {
    let mut ctx = make_ty_ctx();
    let square = register_square(&mut ctx);
    let def_map = def_map_with_module_fn();

    let math_name = intern("math");
    let square_name = intern("square");
    let mut path = Path::from_single(square_name);
    path.kind = glyim_core::path::PathKind::Plain;
    path.segments.insert(
        0,
        PathSegment {
            name: math_name,
            generic_args: None,
        },
    );

    let (hir, body_id) = make_single_body_hir(vec![Expr::Path(path)]);
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
    assert!(diags.is_empty(), "multi-segment fn path should resolve cleanly: {:?}", diags);

    let stmt = &thir_body.stmts[0];
    let expr = match stmt {
        crate::thir::Stmt::Expr { expr, .. } => expr,
        other => panic!("expected Expr statement, got {:?}", other),
    };
    match &expr.kind {
        crate::thir::ExprKind::FnRef(id) => assert_eq!(*id, square),
        other => panic!("expected FnRef, got {:?}", other),
    }
    assert!(
        matches!(ctx.ty_kind(expr.ty), TyKind::FnDef(id, _) if *id == square),
        "FnRef type should be TyKind::FnDef(square, [])"
    );
}

#[test]
fn multi_segment_function_call_yields_registered_return_type() {
    let mut ctx = make_ty_ctx();
    let square = register_square(&mut ctx);
    let def_map = def_map_with_module_fn();

    let math_name = intern("math");
    let square_name = intern("square");
    let mut path = Path::from_single(square_name);
    path.kind = glyim_core::path::PathKind::Plain;
    path.segments.insert(
        0,
        PathSegment {
            name: math_name,
            generic_args: None,
        },
    );

    let (hir, body_id) = make_single_body_hir(vec![
        Expr::Path(path),
        Expr::Call {
            func: ExprId::from_raw(0),
            args: vec![],
        },
    ]);
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
    assert!(diags.is_empty(), "multi-segment fn call should resolve cleanly: {:?}", diags);

    // The call expression is the second statement.
    let stmt = &thir_body.stmts[1];
    let expr = match stmt {
        crate::thir::Stmt::Expr { expr, .. } => expr,
        other => panic!("expected Expr statement, got {:?}", other),
    };
    assert!(
        matches!(ctx.ty_kind(expr.ty), TyKind::Int(glyim_core::primitives::IntTy::I32)),
        "call to math::square() must yield its registered return type i32"
    );
    let _ = square;
}
