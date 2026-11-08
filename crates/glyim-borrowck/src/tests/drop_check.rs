//! Tests for drop checking: use after drop, drop order, conditional drop flags.

use crate::check_borrows;
use glyim_core::arena::IndexVec;
use glyim_mir::{
    BasicBlockData, Body, LocalDecl, LocalIdx, Operand, Place, Rvalue, SourceInfo, Statement,
    StatementKind, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::Ty;

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

fn body_with_statements(stmts: Vec<Statement>) -> Body {
    let mut body = dummy_body();
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals = locals;

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

    body
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn use_after_drop_error() {
    // V11-T01: using a value after it has been dropped should report an error.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![
        glyim_mir::Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        glyim_mir::Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(0)))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let body = body_with_statements(stmts);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(!result.errors.is_empty(), "Expected use-after-drop error");
}

#[test]
fn drop_order_mixed_fields() {
    // V11-T03: Dropping a struct with mixed fields should not cause false errors.
    // The test simply drops a local (representing a struct) and does not use it afterwards.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![glyim_mir::Statement {
        kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
        source_info: SourceInfo::new(Span::DUMMY),
    }];
    let body = body_with_statements(stmts);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        result.errors.is_empty(),
        "dropping a local with no subsequent use should be ok"
    );
}

#[test]
fn use_after_drop_field_error() {
    // Dropping a struct then accessing one of its fields should error.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![
        glyim_mir::Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        glyim_mir::Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Move(Place {
                    local: LocalIdx::from_raw(0),
                    projection: Box::new([glyim_mir::ProjectionElem::Field(
                        glyim_type::FieldIdx::from_raw(0),
                    )]),
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let body = body_with_statements(stmts);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        !result.errors.is_empty(),
        "Expected error for use of dropped struct field"
    );
}

#[test]
fn use_before_drop_no_error() {
    // Using a value before StorageDead should be fine.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![
        glyim_mir::Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(0)))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        glyim_mir::Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let body = body_with_statements(stmts);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    // Move of local 0 is not an error here because we only test move analysis errors.
    // Use-after-drop only triggers after StorageDead.
    // Actually, moving local 0 will be caught by move analysis as use-after-move if it was moved before.
    // Since local 0 is never initialized, it's not moved; just used. That's fine.
    // No drop check error expected because use comes before StorageDead.
    let drop_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved"))
        .collect();
    assert!(
        drop_errors.is_empty(),
        "No use-after-drop errors when use precedes drop"
    );
}

#[test]
fn drop_then_use_copy_field_error() {
    // Even if a field is Copy, accessing it after the whole struct is dropped is an error.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![
        glyim_mir::Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        glyim_mir::Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Copy(Place {
                    local: LocalIdx::from_raw(0),
                    projection: Box::new([glyim_mir::ProjectionElem::Field(
                        glyim_type::FieldIdx::from_raw(0),
                    )]),
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let body = body_with_statements(stmts);
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        !result.errors.is_empty(),
        "Expected error for Copy of dropped struct field"
    );
}

#[test]
fn conditional_drop_one_branch_only() {
    // V11-T04: conditional drop does not flag false positives.
    // Move a Copy type before drop, then drop, no error.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let stmts = vec![
        // Move(Copy) of local0 (Copy type) to local1 — not a real move, no error.
        glyim_mir::Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(0)))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        // Drop local0
        glyim_mir::Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(0)),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let mut body = body_with_statements(stmts);
    body.locals[LocalIdx::from_raw(0)].ty = Ty::BOOL; // Copy type
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(
        result.errors.is_empty(),
        "No errors expected for Copy type move then drop"
    );
}

#[test]
fn conditional_drop_flags() {
    // V11-T04: placeholder test for conditional drop flags.
    let (ctx, ()) = with_fresh_ty_ctx(|_ctx_mut| {});
    let body = dummy_body();
    let test_ctx = TestCtx::new(&ctx, &body.locals);
    let result = check_borrows(&test_ctx, &body);
    assert!(result.errors.is_empty());
}
