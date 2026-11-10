//! `run_program` / `RunOutcome` contract tests (M2.4 slice).
//!
//! Written test-first. The contract pinned here is the **process
//! contract**: `run_program` must behave like the OS + linker do for a
//! real `--emit=exec` binary, so the CLI's `--emit=run`, the test
//! harness's `InterpRunner`, and any future consumer agree on one
//! implementation:
//!
//! * the entry body is the one whose owner's raw `LocalDefId` matches
//!   `entry_main`; `None` falls back to the first body (the harness's
//!   historical behaviour for fixture programs without `main`);
//! * a `main` returning an integer exits `(v & 0xff)` — the same mask
//!   the OS applies to a real process status (`fn main() -> i32 { 300 }`
//!   exits 44 natively; verified in the harness's exit-code notes);
//! * a `main` returning `()` exits 0;
//! * any interpreter error (panic, division by zero, step limit, …)
//!   exits 101 — matching `glyim_panic`'s `exit(101)` — and appends a
//!   `thread 'main' panicked: …` line to stderr, so `check-stderr`
//!   fixtures mean the same thing on both backends;
//! * stdout/stderr bytes captured from FFI writes are returned even
//!   when the program panics (the TEST-7 fidelity rule).

use crate::run::{RunKind, run_program};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyCtx, TyCtxMut, TyKind};
use std::sync::Arc;

/// Local 0 is the return place; everything a body "returns" lands there.
fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// A body whose `main` returns the constant `value`: the equivalent of
/// `fn main() -> i32 { <value> }` at the MIR level.
fn body_returning(value: MirConst, owner_raw: u32) -> Body {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    body.locals = IndexVec::from_raw(vec![local_decl(value.ty, Mutability::Not)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(value)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

/// A body that divides two constants, tripping the interpreter's
/// `DivisionByZero` error (the simplest way to exercise the panic path
/// from a hand-built body).
fn body_div_by_zero(owner_raw: u32, tcx: &mut TyCtxMut) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let int_const = |v: i128| MirConst {
        kind: MirConstKind::Int(v),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let mut body = Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(owner_raw),
    ));
    body.locals = IndexVec::from_raw(vec![local_decl(i32_ty, Mutability::Not)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::BinaryOp(
                    BinOp::Div,
                    Box::new((
                        Operand::Constant(int_const(5)),
                        Operand::Constant(int_const(0)),
                    )),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

fn int_const(tcx: &mut TyCtxMut, v: i128) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(v),
        ty: tcx.mk_ty(TyKind::Int(IntTy::I32)),
        span: Span::DUMMY,
    }
}

#[test]
fn run_program_runs_the_declared_entry_body() {
    let mut tcx_mut = test_ty_ctx();
    let one = body_returning(int_const(&mut tcx_mut, 1), 10);
    let seven = body_returning(int_const(&mut tcx_mut, 7), 20);
    let tcx: TyCtx = tcx_mut.freeze();
    let bodies = vec![Arc::new(one), Arc::new(seven)];

    // `entry_main = Some(20)` must select the body owned by raw id 20,
    // regardless of its position in the slice.
    let outcome = run_program(&bodies, &tcx, Some(20));
    assert_eq!(outcome.kind, RunKind::Completed);
    assert_eq!(outcome.exit_code, 7);
    assert!(outcome.stderr.is_empty());
}

#[test]
fn run_program_without_entry_falls_back_to_the_first_body() {
    let mut tcx_mut = test_ty_ctx();
    let one = body_returning(int_const(&mut tcx_mut, 1), 10);
    let seven = body_returning(int_const(&mut tcx_mut, 7), 20);
    let tcx: TyCtx = tcx_mut.freeze();
    let bodies = vec![Arc::new(one), Arc::new(seven)];

    // The harness's historical contract: fixtures without a `main` run
    // their first body instead of erroring.
    let outcome = run_program(&bodies, &tcx, None);
    assert_eq!(outcome.kind, RunKind::Completed);
    assert_eq!(outcome.exit_code, 1);
}

#[test]
fn run_program_masks_exit_code_to_u8() {
    let mut tcx_mut = test_ty_ctx();
    // `fn main() -> i32 { 300 }` exits 44 natively (status & 0xff); the
    // interpreter must reproduce that fidelity for run-pass fixtures.
    let big_signed = body_returning(int_const(&mut tcx_mut, 300), 0);
    let tcx: TyCtx = tcx_mut.freeze();
    let outcome = run_program(&[Arc::new(big_signed)], &tcx, None);
    assert_eq!(outcome.exit_code, 44);

    let mut tcx_mut = test_ty_ctx();
    let big_unsigned = body_returning(
        MirConst {
            kind: MirConstKind::Uint(513),
            ty: tcx_mut.mk_ty(TyKind::Uint(UintTy::U32)),
            span: Span::DUMMY,
        },
        0,
    );
    let tcx: TyCtx = tcx_mut.freeze();
    let outcome = run_program(&[Arc::new(big_unsigned)], &tcx, None);
    assert_eq!(outcome.exit_code, 1);
}

#[test]
fn run_program_unit_main_exits_zero() {
    let unit = body_returning(
        MirConst {
            kind: MirConstKind::Unit,
            ty: Ty::UNIT,
            span: Span::DUMMY,
        },
        0,
    );
    // A frozen context is still required by `run_program` even though the
    // unit-returning body touches no interpreter types.
    let tcx: TyCtx = test_ty_ctx().freeze();
    let outcome = run_program(&[Arc::new(unit)], &tcx, None);
    assert_eq!(outcome.kind, RunKind::Completed);
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stderr.is_empty());
}

#[test]
fn run_program_panics_exit_101_with_canonical_stderr() {
    let mut tcx_mut = test_ty_ctx();
    let boom = body_div_by_zero(0, &mut tcx_mut);
    let tcx: TyCtx = tcx_mut.freeze();
    let outcome = run_program(&[Arc::new(boom)], &tcx, None);

    // `glyim_panic` exits 101 without unwinding; any interpreter error is
    // the in-process equivalent and must surface the same exit code.
    assert_eq!(outcome.kind, RunKind::Panicked);
    assert_eq!(outcome.exit_code, 101);
    let stderr = String::from_utf8_lossy(&outcome.stderr).into_owned();
    assert!(
        stderr.contains("thread 'main' panicked:"),
        "stderr must carry the canonical panic line, got: {stderr:?}"
    );
    assert!(
        stderr.contains("attempt to divide by zero"),
        "stderr must carry the interp error text, got: {stderr:?}"
    );
}

#[test]
fn run_program_on_empty_body_list_is_a_clean_zero_exit() {
    let mut tcx_mut = test_ty_ctx();
    let _ = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let tcx: TyCtx = tcx_mut.freeze();
    let outcome = run_program(&[], &tcx, None);
    assert_eq!(outcome.kind, RunKind::Completed);
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.stdout.is_empty());
    assert!(outcome.stderr.is_empty());
}
