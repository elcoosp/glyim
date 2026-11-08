//! Tests for interior mutability recognition: Cell/RefCell.
//!
//! These tests verify that shared borrows of interior-mutable types (e.g., Cell)
//! allow writes through the shared reference, whereas non-interior-mutable types
//! reject such writes.

use crate::check_borrows;
use glyim_core::AdtId;
use glyim_core::arena::IndexVec;
use glyim_mir::{
    BasicBlockData, Body, BorrowKind, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place,
    Rvalue, SourceInfo, Statement, StatementKind, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Ty, TyKind};

// ---------------------------------------------------------------------------
// Simple mock context implementing crate::BorrowckCtx
// ---------------------------------------------------------------------------

use glyim_mir::LocalDecl as MirLocalDecl;
use glyim_type::TyCtx;

struct TestCtx<'a> {
    ty_ctx: &'a TyCtx,
    locals: &'a IndexVec<LocalIdx, MirLocalDecl>,
}

impl<'a> TestCtx<'a> {
    fn new(ty_ctx: &'a TyCtx, locals: &'a IndexVec<LocalIdx, MirLocalDecl>) -> Self {
        Self { ty_ctx, locals }
    }
}

impl crate::BorrowckCtx for TestCtx<'_> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }

    fn local_decl(&self, local: LocalIdx) -> &MirLocalDecl {
        &self.locals[local]
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dummy_body() -> Body {
    Body {
        owner: glyim_core::DefId::new(
            glyim_core::CrateId::from_raw(0),
            glyim_core::LocalDefId::from_raw(0),
        ),
        basic_blocks: IndexVec::new(),
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Build a body where:
/// - local 0: the interior-mutable ADT
/// - local 1: shared borrow of local 0 (active)
/// - then a write to local 0 while the shared borrow is still live.
/// - `interior_mutable` determines whether the ADT is marked as interior-mutable.
fn build_interior_mut_body(interior_mutable: bool) -> (TyCtx, Body) {
    let (ctx, (_adt_id, adt_ty)) = with_fresh_ty_ctx(|ctx_mut| {
        // Create a dummy ADT type: struct Dummy(i32) with 1 field.
        let adt_id = AdtId::from_raw(0);
        // Mark interior mutability BEFORE creating the type so flags are correct.
        if interior_mutable {
            ctx_mut.mark_adt_interior_mutable(adt_id);
        }
        let substs = ctx_mut.intern_substitution(vec![]);
        let adt_ty = ctx_mut.mk_ty(TyKind::Adt(adt_id, substs));
        (adt_id, adt_ty)
    });

    let mut body = dummy_body();
    let mut locals = IndexVec::new();
    // local 0: the ADT
    locals.push(LocalDecl {
        ty: adt_ty,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    // local 1: shared reference to local 0
    locals.push(LocalDecl {
        ty: Ty::UNIT, // placeholder, not used for checking
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals = locals;

    let stmts = vec![
        // Statement 0: create shared borrow of local 0, storing in local 1
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Ref(Place::new(LocalIdx::from_raw(0)), BorrowKind::Shared),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        // Statement 1: write to local 0 (assign unit) while local 1 is live
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        // Statement 2: use local 1 to keep it live (read it)
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];

    let bb = BasicBlockData {
        statements: stmts,
        terminator: glyim_mir::Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut blocks = IndexVec::new();
    blocks.push(bb);
    body.basic_blocks = blocks;

    (ctx, body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn cell_mutation_through_shared_ref() {
    // V11-T02: Cell<i32> allows mutation through shared reference.
    // Without interior mutability, writing through a shared borrow should error.
    let (ctx, body) = build_interior_mut_body(false);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        !result.errors.is_empty(),
        "Expected error for write through shared borrow of non-interior-mutable type"
    );
}

#[test]
fn interior_mutability_flag_is_set() {
    // Verify that marking an ADT as interior mutable sets HAS_INTERIOR_MUTABILITY on the type.
    let (ctx, adt_ty) = with_fresh_ty_ctx(|ctx_mut| {
        let adt_id = AdtId::from_raw(0);
        ctx_mut.mark_adt_interior_mutable(adt_id);
        let substs = ctx_mut.intern_substitution(vec![]);
        ctx_mut.mk_ty(TyKind::Adt(adt_id, substs))
    });
    let flags = ctx.ty_flags(adt_ty);
    assert!(
        flags.contains(glyim_type::TypeFlags::HAS_INTERIOR_MUTABILITY),
        "Expected HAS_INTERIOR_MUTABILITY flag"
    );
}

#[test]
fn non_interior_mutable_write_through_shared_errors() {
    // Without interior mutability, writing through a shared borrow should error.
    let (ctx, body) = build_interior_mut_body(false);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        !result.errors.is_empty(),
        "Expected error for write through shared borrow"
    );
}

#[test]
fn interior_mutable_flag_non_adt_ignored() {
    // For non-ADT types, interior mutability flag should never be set.
    let (ctx, unit_ty) = with_fresh_ty_ctx(|ctx_mut| ctx_mut.unit_ty());
    let flags = ctx.ty_flags(unit_ty);
    assert!(!flags.contains(glyim_type::TypeFlags::HAS_INTERIOR_MUTABILITY));
    let bool_ty = Ty::BOOL;
    let flags = ctx.ty_flags(bool_ty);
    assert!(!flags.contains(glyim_type::TypeFlags::HAS_INTERIOR_MUTABILITY));
}
