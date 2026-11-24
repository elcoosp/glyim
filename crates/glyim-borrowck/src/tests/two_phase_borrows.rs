//! Tests for two-phase borrow support (Stream V09).
//!
//! Two-phase borrows allow a mutable borrow to start in a "reservation"
//! phase (acting as a shared borrow) and only become "activated" at the
//! call site. This enables patterns like `vec.push(vec.len())` where the
//! mutable borrow of `vec` for `push` does not conflict with the shared
//! read of `vec.len()` that occurs in the arguments.

use crate::{BorrowckCtx, check_borrows};
use glyim_core::{CrateId, DefId, IndexVec, LocalDefId, Mutability};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, LocalIdx, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::{assert_has_errors, assert_no_errors, with_fresh_ty_ctx};
use glyim_type::Ty;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct LocalMockBorrowckCtx {
    ty_ctx: glyim_type::TyCtx,
    body: Body,
}

impl BorrowckCtx for LocalMockBorrowckCtx {
    fn ty_ctx(&self) -> &glyim_type::TyCtx {
        &self.ty_ctx
    }
    fn local_decl(&self, local: LocalIdx) -> &glyim_mir::LocalDecl {
        &self.body.locals[local]
    }
    fn is_copy(&self, _ty: Ty) -> bool {
        false
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}

fn local_decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn make_body(statements: Vec<Statement>, locals: Vec<LocalDecl>) -> Body {
    let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(def_id);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    })]);
    body.basic_blocks[BasicBlockIdx::from_raw(0)].statements = statements;
    body.locals = IndexVec::from_raw(locals);
    body
}

fn make_body_multi_blocks(locals: Vec<LocalDecl>, blocks: Vec<BasicBlockData>) -> Body {
    let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(def_id);
    body.basic_blocks = IndexVec::<BasicBlockIdx, _>::from_raw(blocks);
    body.locals = IndexVec::<LocalIdx, _>::from_raw(locals);
    body
}

fn make_ref_ty(ctx_mut: &mut glyim_type::TyCtxMut, inner: Ty, mutable: bool) -> Ty {
    let mutability = if mutable {
        Mutability::Mut
    } else {
        Mutability::Not
    };
    ctx_mut.mk_ref(glyim_type::Region::Erased, inner, mutability)
}

fn assign_borrow(dest: LocalIdx, place: Place, kind: BorrowKind) -> Statement {
    Statement {
        kind: StatementKind::Assign(Place::new(dest), Rvalue::Ref(place, kind)),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn use_local(dest: LocalIdx, local: LocalIdx) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Copy(Place::new(local))),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn make_two_phase_mut() -> BorrowKind {
    BorrowKind::Mut {
        allow_two_phase_borrow: true,
    }
}

fn make_one_phase_mut() -> BorrowKind {
    BorrowKind::Mut {
        allow_two_phase_borrow: false,
    }
}

fn dummy_terminator_goto(target: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(target),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn dummy_terminator_return() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

// ===========================================================================
// V09-T01: vec.push(vec.len()) pattern — two-phase borrow with intervening
// shared read during reservation — should NOT error
// ===========================================================================

#[test]
fn t01_two_phase_borrow_with_intervening_shared_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T02: Two-phase borrow, shared borrow during reservation — no error
// ===========================================================================

#[test]
fn t02_two_phase_reservation_allows_shared_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T03: Activation point conflict — shared borrow after activation → error
// ===========================================================================

#[test]
fn t03_activation_conflict_with_shared_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T04: Reservation conflict with other mutable borrow → error
// ===========================================================================

#[test]
fn t04_reservation_conflict_with_other_mutable() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T05: One-phase mut borrow conflicts with shared — baseline
// ===========================================================================

#[test]
fn t05_one_phase_mut_conflicts_with_shared() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_one_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T06: Two-phase borrow never activated — no conflict
// ===========================================================================

#[test]
fn t06_two_phase_borrow_never_activated_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T07: Read of borrowed place during reservation — no conflict
// ===========================================================================

#[test]
fn t07_two_phase_reservation_allows_read_of_borrowed_place() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T08: Write to borrowed place during reservation — error
// ===========================================================================

#[test]
fn t08_two_phase_reservation_write_to_borrowed_place_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(3)))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T09: Two-phase reservation conflicts with Unique borrow — error
// ===========================================================================

#[test]
fn t09_two_phase_reservation_conflicts_with_unique_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Unique,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T10: Two two-phase borrows on different places — no conflict
// ===========================================================================

#[test]
fn t10_two_phase_borrows_different_places_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(2)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(4)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T11: Multiple shared borrows during reservation — no conflict
// ===========================================================================

#[test]
fn t11_two_phase_reservation_multiple_shared_borrows() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(7), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T12: Shared ref dies before two-phase activation — no conflict
// ===========================================================================

#[test]
fn t12_shared_ref_dies_before_two_phase_activation() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T13: One-phase mut after shared expires — no conflict
// ===========================================================================

#[test]
fn t13_one_phase_mut_after_shared_expires_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_one_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T14: Two-phase borrow across basic blocks — conservatively activated
// ===========================================================================

#[test]
fn t14_two_phase_cross_block_conservatively_activated() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let mut block0 = BasicBlockData::new(dummy_terminator_goto(1));
        block0.statements.push(assign_borrow(
            LocalIdx::from_raw(2),
            Place::new(LocalIdx::from_raw(1)),
            make_two_phase_mut(),
        ));
        let mut block1 = BasicBlockData::new(dummy_terminator_goto(2));
        block1.statements.push(assign_borrow(
            LocalIdx::from_raw(3),
            Place::new(LocalIdx::from_raw(1)),
            BorrowKind::Shared,
        ));
        let mut block2 = BasicBlockData::new(dummy_terminator_return());
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)));
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(2)));
        make_body_multi_blocks(locals, vec![block0, block1, block2])
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T15: Two-phase activated then shared in same block — error
// ===========================================================================

#[test]
fn t15_two_phase_activated_then_shared_in_same_block_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T16: Two two-phase mut borrows same place — error
// ===========================================================================

#[test]
fn t16_two_two_phase_mut_borrows_same_place_error() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T17: Shared borrow before two-phase mut — error (active shared blocks mut)
// ===========================================================================

#[test]
fn t17_shared_borrow_before_two_phase_mut_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(3)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T18: Read and shared borrow during reservation — no conflict
// ===========================================================================

#[test]
fn t18_two_phase_reservation_read_and_shared_borrow() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(1)),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T19: Immediate activation — no reservation window
// ===========================================================================

#[test]
fn t19_two_phase_immediate_activation_no_reservation_window() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T20: Long reservation window with multiple reads
// ===========================================================================

#[test]
fn t20_two_phase_long_reservation_window_multiple_reads() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(1)),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(3)),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(7), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(8), LocalIdx::from_raw(1)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T21: Discriminant read during reservation — no conflict
// ===========================================================================

#[test]
fn t21_two_phase_reservation_discriminant_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Discriminant(Place::new(LocalIdx::from_raw(1))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T22: Len read during reservation — no conflict
// ===========================================================================

#[test]
fn t22_two_phase_reservation_len_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Len(Place::new(LocalIdx::from_raw(1))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T23: Shared ref dies then two-phase activated — no conflict
// ===========================================================================

#[test]
fn t23_shared_dies_then_two_phase_activated_no_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T24: Copy-use of dest_local activates the two-phase borrow.
// After activation, a shared borrow of the same place conflicts.
// Note: Operand::Move kills the source local, making the loan inactive
// in our liveness model. Copy keeps dest_local live so the conflict
// is detected — this is a known v0.1.0 simplification.
// ===========================================================================

#[test]
fn t24_two_phase_activation_via_copy_use() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            // local_2 = &mut local_1 (two-phase reservation)
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            // local_3 = *local_2 (Copy use of local_2 — activates, local_2 stays live)
            use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2)),
            // local_4 = &local_1 (shared borrow after activation — conflict!)
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(4)),
            use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}

// ===========================================================================
// V09-T25: BinaryOp reading borrowed place during reservation
// ===========================================================================

#[test]
fn t25_two_phase_reservation_binary_op_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::BinaryOp(
                        glyim_core::BinOp::Add,
                        [
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        ],
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T26: UnaryOp reading borrowed place during reservation
// ===========================================================================

#[test]
fn t26_two_phase_reservation_unary_op_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::UnaryOp(
                        glyim_core::UnOp::Neg,
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T27: Two two-phase borrows on different places with shared reads
// ===========================================================================

#[test]
fn t27_two_phase_different_places_each_with_shared_reads() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(3),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(4),
                Place::new(LocalIdx::from_raw(2)),
                make_two_phase_mut(),
            ),
            assign_borrow(
                LocalIdx::from_raw(5),
                Place::new(LocalIdx::from_raw(1)),
                BorrowKind::Shared,
            ),
            assign_borrow(
                LocalIdx::from_raw(6),
                Place::new(LocalIdx::from_raw(2)),
                BorrowKind::Shared,
            ),
            use_local(LocalIdx::from_raw(7), LocalIdx::from_raw(5)),
            use_local(LocalIdx::from_raw(8), LocalIdx::from_raw(6)),
            use_local(LocalIdx::from_raw(9), LocalIdx::from_raw(3)),
            use_local(LocalIdx::from_raw(10), LocalIdx::from_raw(4)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// ===========================================================================
// V09-T28: Cast reading borrowed place during reservation
// ===========================================================================

#[test]
fn t28_two_phase_reservation_cast_read() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let i64_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I64));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i64_ty),
            local_decl(i32_ty),
        ];
        let stmts = vec![
            assign_borrow(
                LocalIdx::from_raw(2),
                Place::new(LocalIdx::from_raw(1)),
                make_two_phase_mut(),
            ),
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Cast(
                        glyim_mir::CastKind::IntToInt,
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        i64_ty,
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(2)),
        ];
        make_body(stmts, locals)
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

// === Cross-block two-phase borrow tests ===

#[test]
fn t14_two_phase_cross_block_reservation_extends() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let mut block0 = BasicBlockData::new(dummy_terminator_goto(1));
        block0.statements.push(assign_borrow(
            LocalIdx::from_raw(2),
            Place::new(LocalIdx::from_raw(1)),
            make_two_phase_mut(),
        ));
        let mut block1 = BasicBlockData::new(dummy_terminator_goto(2));
        block1.statements.push(assign_borrow(
            LocalIdx::from_raw(3),
            Place::new(LocalIdx::from_raw(1)),
            BorrowKind::Shared,
        ));
        let mut block2 = BasicBlockData::new(dummy_terminator_return());
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(4), LocalIdx::from_raw(3)));
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(2)));
        make_body_multi_blocks(locals, vec![block0, block1, block2])
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_no_errors(&result.errors);
}

#[test]
fn t15_two_phase_cross_block_activation_conflict() {
    let (ctx, body) = with_fresh_ty_ctx(|ctx_mut| {
        let unit = ctx_mut.unit_ty();
        let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
        let mut_ref_ty = make_ref_ty(ctx_mut, i32_ty, true);
        let shared_ref_ty = make_ref_ty(ctx_mut, i32_ty, false);
        let locals = vec![
            local_decl(unit),
            local_decl(i32_ty),
            local_decl(mut_ref_ty),
            local_decl(i32_ty),
            local_decl(shared_ref_ty),
            local_decl(i32_ty),
            local_decl(i32_ty),
        ];
        let mut block0 = BasicBlockData::new(dummy_terminator_goto(1));
        block0.statements.push(assign_borrow(
            LocalIdx::from_raw(2),
            Place::new(LocalIdx::from_raw(1)),
            make_two_phase_mut(),
        ));
        block0
            .statements
            .push(use_local(LocalIdx::from_raw(3), LocalIdx::from_raw(2))); // activates
        let mut block1 = BasicBlockData::new(dummy_terminator_goto(2));
        block1.statements.push(assign_borrow(
            LocalIdx::from_raw(4),
            Place::new(LocalIdx::from_raw(1)),
            BorrowKind::Shared,
        ));
        let mut block2 = BasicBlockData::new(dummy_terminator_return());
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(5), LocalIdx::from_raw(4)));
        block2
            .statements
            .push(use_local(LocalIdx::from_raw(6), LocalIdx::from_raw(2)));
        make_body_multi_blocks(locals, vec![block0, block1, block2])
    });
    let mock = LocalMockBorrowckCtx { ty_ctx: ctx, body };
    let result = check_borrows(&mock, &mock.body);
    assert_has_errors(&result.errors);
}
