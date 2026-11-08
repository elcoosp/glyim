//! S06-T04: Drop glue scanning and Drop terminator implementation.

use crate::{MonoCtx, MonoItem};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::*;
use glyim_core::primitives::Mutability;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::*;
use std::sync::Arc;

/// S06-T04: Drop glue for `Vec<T>` calls `drop_in_place` for `T`.
///
/// When a MIR body contains a `Drop` terminator, the monomorphization
/// collector should enqueue a `MonoItem::DropGlue` for the dropped type.
#[test]
fn drop_terminator_enqueues_drop_glue() {
    let i32_ty = {
        let mut ctx = glyim_test::test_ty_ctx();
        ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32))
    };
    let mut mono = MonoCtx::new();

    // Create a MIR body with a Drop terminator for an i32 local
    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let drop_term = Terminator {
        kind: TerminatorKind::Drop {
            place: Place::new(LocalIdx::from_raw(1)),
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let ret_term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: drop_term,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: ret_term,
        is_cleanup: false,
    });

    let body = Arc::new(Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    });

    // Provide a trivial drop_glue_body implementation
    let drop_glue_fn = |_ty: Ty| -> Arc<Body> {
        Arc::new(Body::dummy(DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(0),
        )))
    };

    // Provide a trivial mir_bodies implementation
    let mir_bodies_fn = |_def_id: DefId, _substs: &Substitution| -> Arc<Body> {
        Arc::new(Body::dummy(DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(0),
        )))
    };

    mono.collect(&[], &mir_bodies_fn, &drop_glue_fn);

    // Now test: collect a start item that references the body with Drop
    let start_fn = MonoItem::Fn {
        def_id: FnDefId::from_raw(0),
        substs: Substitution::empty(),
    };

    let mut mono2 = MonoCtx::new();
    // We need to provide the body for the start function
    let body_clone = body.clone();
    let mir_fn = move |_def_id: DefId, _substs: &Substitution| -> Arc<Body> { body_clone.clone() };
    let drop_fn = |_ty: Ty| -> Arc<Body> {
        Arc::new(Body::dummy(DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(0),
        )))
    };

    mono2.collect(&[start_fn], &mir_fn, &drop_fn);

    // After collection, there should be a DropGlue mono item for i32
    let has_drop_glue = mono2
        .items()
        .iter()
        .any(|item| matches!(item.item, MonoItem::DropGlue { .. }));
    assert!(
        has_drop_glue,
        "expected DropGlue mono item for i32 type after collecting body with Drop terminator"
    );
}

/// Verify that the Drop terminator's `scan_terminator` no longer emits a STUB warning.
/// We can't easily check tracing output, but we verify that `scan_terminator`
/// handles Drop by enqueuing the appropriate mono item.
#[test]
fn drop_terminator_scan_no_stub() {
    // This test validates structurally that MonoCtx::scan_terminator
    // handles Drop by looking for the DropGlue in collected items.
    let struct_ty = {
        let mut ctx = glyim_test::test_ty_ctx();
        let adt_id = AdtId::from_raw(42);
        let substs = ctx.intern_substitution(vec![]);
        ctx.mk_ty(TyKind::Adt(adt_id, substs))
    };

    let owner = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: struct_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let drop_term = Terminator {
        kind: TerminatorKind::Drop {
            place: Place::new(LocalIdx::from_raw(1)),
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let ret_term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: drop_term,
        is_cleanup: false,
    });
    basic_blocks.push(BasicBlockData {
        statements: vec![],
        terminator: ret_term,
        is_cleanup: false,
    });

    let body = Arc::new(Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    });

    let start_fn = MonoItem::Fn {
        def_id: FnDefId::from_raw(0),
        substs: Substitution::empty(),
    };

    let body_clone = body.clone();
    let mir_fn = move |_def_id: DefId, _substs: &Substitution| -> Arc<Body> { body_clone.clone() };
    let drop_fn = |_ty: Ty| -> Arc<Body> {
        Arc::new(Body::dummy(DefId::new(
            CrateId::from_raw(0),
            LocalDefId::from_raw(0),
        )))
    };

    let mut mono = MonoCtx::new();
    mono.collect(&[start_fn], &mir_fn, &drop_fn);

    let drop_glue_items: Vec<_> = mono
        .items()
        .iter()
        .filter(|item| matches!(item.item, MonoItem::DropGlue { .. }))
        .collect();
    assert!(
        !drop_glue_items.is_empty(),
        "expected DropGlue mono item(s) after scanning body with Drop terminator"
    );
}
