//! Legal-program corpus for the borrow checker.
//!
//! The audit noted that a naive drop-after-move fix (MIR-24) was reverted
//! because it false-positived on *legal* programs — "drop of an
//! already-moved value is OK". Without tests that pin the legal cases, such a
//! fix gets re-attempted and re-broken. This file asserts, for a set of
//! programs that Rust accepts, that `check_borrows` reports **zero** errors.
//!
//! Every test here must FAIL if the checker over-rejects. The companion
//! illegal cases live in `move_tests.rs` / `drop_check.rs` and assert the
//! opposite (`!errors.is_empty()`), so the two together stop both
//! over- and under-rejection regressions.

use crate::{BorrowckCtx, check_borrows};
use glyim_core::arena::IndexVec;
use glyim_core::primitives::Mutability;
use glyim_core::{CrateId, DefId, LocalDefId};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, BorrowKind, LocalDecl, LocalIdx, Operand, Place, Rvalue,
    SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Ty, TyCtx};

struct TestCtx<'a> {
    ty_ctx: &'a TyCtx,
    locals: &'a IndexVec<LocalIdx, LocalDecl>,
}

impl<'a> TestCtx<'a> {
    fn new(ty_ctx: &'a TyCtx, locals: &'a IndexVec<LocalIdx, LocalDecl>) -> Self {
        Self { ty_ctx, locals }
    }
}

impl<'a> BorrowckCtx for TestCtx<'a> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }
    fn local_decl(&self, local: LocalIdx) -> &LocalDecl {
        &self.locals[local]
    }
    fn local_name(&self, idx: LocalIdx) -> String {
        format!("_{}", idx.to_raw())
    }
}

fn decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn assign(local: LocalIdx, rv: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(Place::new(local), rv),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn move_stmt(dst: LocalIdx, src: LocalIdx) -> Statement {
    assign(dst, Rvalue::Use(Operand::Move(Place::new(src))))
}

fn copy_stmt(dst: LocalIdx, src: LocalIdx) -> Statement {
    assign(dst, Rvalue::Use(Operand::Copy(Place::new(src))))
}

fn drop_term(place: LocalIdx, target: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::Drop {
            place: Place::new(place),
            target: BasicBlockIdx::from_raw(target),
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn ret_term() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Run `check_borrows` and assert no errors — the legal-program oracle.
fn assert_accepts(ctx: &TyCtx, body: &Body, why: &str) {
    let mock = TestCtx::new(ctx, &body.locals);
    let result = check_borrows(&mock, body);
    assert!(
        result.errors.is_empty(),
        "legal program was rejected ({why}): {:?}",
        result.errors
    );
}

/// MIR-24 case: move a value, then `Drop` runs on the (now-moved) local at
/// scope exit. Rust accepts this — the drop flag suppresses the destructor.
/// A naive "every Drop must be preceded by an init" check rejects it.
#[test]
fn drop_after_move_is_accepted() {
    let (ctx, _) = with_fresh_ty_ctx(|_| {});
    let ty = Ty::UNIT;

    // Blocks: 0 = move + drop, 1 = return. Both locals are i32 (no real
    // destructor, but the *shape* is what a needs-drop local produces).
    let blocks = vec![
        BasicBlockData {
            statements: vec![
                move_stmt(LocalIdx::from_raw(1), LocalIdx::from_raw(2)),
            ],
            terminator: drop_term(LocalIdx::from_raw(2), 1),
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: ret_term(),
            is_cleanup: false,
        },
    ];
    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(blocks),
        locals: IndexVec::from_raw(vec![
            decl(ty),
            decl(ty),
            decl(ty),
        ]),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    assert_accepts(&ctx, &body, "drop of an already-moved local is legal");
}

/// Move out of one local, then read a *different* local: accepted.
#[test]
fn move_one_local_read_another_is_accepted() {
    let (ctx, _) = with_fresh_ty_ctx(|_| {});
    let ty = Ty::UNIT;

    let blocks = vec![BasicBlockData {
        statements: vec![
            move_stmt(LocalIdx::from_raw(0), LocalIdx::from_raw(2)),
            copy_stmt(LocalIdx::from_raw(1), LocalIdx::from_raw(3)),
        ],
        terminator: ret_term(),
        is_cleanup: false,
    }];
    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(blocks),
        locals: IndexVec::from_raw(vec![decl(ty); 4]),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    assert_accepts(&ctx, &body, "reading an unmoved local after a sibling move");
}

/// A shared borrow released before a mutable borrow of the same place is
/// accepted (the two loans do not overlap in liveness).
#[test]
fn sequential_borrows_of_same_place_are_accepted() {
    let (ctx, _) = with_fresh_ty_ctx(|_| {});
    let ty = Ty::UNIT;

    let blocks = vec![BasicBlockData {
        statements: vec![
            assign(
                LocalIdx::from_raw(1),
                Rvalue::Ref(Place::new(LocalIdx::from_raw(0)), BorrowKind::Shared),
            ),
            assign(
                LocalIdx::from_raw(2),
                Rvalue::Ref(
                    Place::new(LocalIdx::from_raw(0)),
                    BorrowKind::Mut {
                        allow_two_phase_borrow: false,
                    },
                ),
            ),
        ],
        terminator: ret_term(),
        is_cleanup: false,
    }];
    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(blocks),
        locals: IndexVec::from_raw(vec![decl(ty); 3]),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    assert_accepts(&ctx, &body, "shared then mutable borrow, no overlap");
}

/// Two simultaneous shared borrows of the same place: accepted.
#[test]
fn two_shared_borrows_are_accepted() {
    let (ctx, _) = with_fresh_ty_ctx(|_| {});
    let ty = Ty::UNIT;

    let blocks = vec![BasicBlockData {
        statements: vec![
            assign(
                LocalIdx::from_raw(1),
                Rvalue::Ref(Place::new(LocalIdx::from_raw(0)), BorrowKind::Shared),
            ),
            assign(
                LocalIdx::from_raw(2),
                Rvalue::Ref(Place::new(LocalIdx::from_raw(0)), BorrowKind::Shared),
            ),
        ],
        terminator: ret_term(),
        is_cleanup: false,
    }];
    let body = Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: IndexVec::from_raw(blocks),
        locals: IndexVec::from_raw(vec![decl(ty); 3]),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    };
    assert_accepts(&ctx, &body, "two shared borrows may overlap");
}
