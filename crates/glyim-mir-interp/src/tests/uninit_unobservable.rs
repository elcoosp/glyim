//! Uninitialized-read semantics for unobservable types (`()` / `!`), and
//! the `StorageLive` back-edge contract.
//!
//! The opt-level pass-plan seam (`glyim-opt::PassPlan::for_opt_level(0)`)
//! stopped erasing lowering's raw statement shapes before the interpreter
//! sees them. Two shapes that the full pipeline's DCE/cfg-simplify used to
//! mask, and that the LLVM backend shrugs off, are now first-class interp
//! contracts pinned here:
//!
//! 1. `StorageLive` re-executed on a loop back-edge must NOT wipe the
//!    loop-carried value (`read from uninitialized local` used to fire on
//!    `let mut i = 0; loop { if i == 3 { break; } i = i + 1; }`);
//! 2. a `()`/`!`-typed local read before any write yields `Unit` — the
//!    dead-temp chains lowering emits at conditional joins and loop-desugar
//!    phantoms — while an INHABITED type still panics (the genuine
//!    read-before-write guard is intact).

use crate::*;
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand, Place,
    Rvalue, SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0))
}

fn decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn const_i32(v: i32, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(i128::from(v)),
        ty,
        span: Span::DUMMY,
    })
}

fn assign(place: u32, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(place as usize)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn storage_live(local: u32) -> Statement {
    Statement {
        kind: StatementKind::StorageLive(LocalIdx::from_usize(local as usize)),
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn block(stmts: Vec<Statement>, term: Terminator) -> BasicBlockData {
    BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    }
}

fn goto(target: u32) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_usize(target as usize),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn return_term() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// The `baseline_loop_break.g` shape: `StorageLive(_2)` sits in the loop
/// HEADER (`bb1`) and re-executes on every back-edge; the carried value must
/// survive so the `break` condition eventually fires.
///
/// ```text
/// bb0: StorageLive(_1); _1 = 0; _2 = Move(_1); goto bb1
/// bb1: StorageLive(_2); _2 = _2 + 1; if _2 >= 4 { goto bb2 } else { goto bb1 }
/// bb2: _0 = Copy(_2); return
/// ```
#[test]
fn storage_live_on_back_edge_preserves_loop_carried_value() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let tcx = tcx_mut.freeze();

    let cond_ty = tcx.bool_ty();
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![
        decl(i32_ty),
        decl(i32_ty),
        decl(i32_ty),
        decl(cond_ty),
    ]);
    body.basic_blocks = IndexVec::from_vec(vec![
        block(
            vec![
                storage_live(1),
                assign(1, Rvalue::Use(const_i32(0, i32_ty))),
                assign(
                    2,
                    Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_usize(1)))),
                ),
            ],
            goto(1),
        ),
        block(
            vec![
                storage_live(2),
                assign(
                    2,
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        [
                            Operand::Copy(Place::new(LocalIdx::from_usize(2))),
                            const_i32(1, i32_ty),
                        ],
                    ),
                ),
            ],
            Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_usize(2))),
                    switch_ty: i32_ty,
                    targets: glyim_mir::SwitchTargets::new(
                        vec![(4, BasicBlockIdx::from_usize(2))].into_boxed_slice(),
                        BasicBlockIdx::from_usize(1),
                    ),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ),
        block(
            vec![assign(
                0,
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_usize(2)))),
            )],
            return_term(),
        ),
    ]);

    let mut interp = Interpreter::new(&tcx);
    interp
        .run_body(&body)
        .expect("re-StorageLive on the back-edge must not wipe the carried value");
    let counter = interp
        .get_local_value(LocalIdx::from_usize(2))
        .expect("counter local must be initialized at exit");
    assert_eq!(
        counter,
        &InterpValue::Int(4),
        "the loop must count 1..=4 on the carried value, not restart each iteration"
    );
}

/// A `()`-typed temp read at a join it only reaches from one predecessor
/// (the compound-statement shape) yields `Unit` instead of panicking.
#[test]
fn uninit_unit_temp_read_yields_unit() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let tcx = tcx_mut.freeze();

    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![decl(i32_ty), decl(i32_ty), decl(Ty::UNIT)]);
    body.basic_blocks = IndexVec::from_vec(vec![
        block(
            vec![assign(1, Rvalue::Use(const_i32(1, i32_ty)))],
            Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_usize(1))),
                    switch_ty: i32_ty,
                    targets: glyim_mir::SwitchTargets::new(
                        vec![(1, BasicBlockIdx::from_usize(2))].into_boxed_slice(),
                        BasicBlockIdx::from_usize(1),
                    ),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ),
        block(
            vec![assign(
                2,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            )],
            goto(3),
        ),
        block(vec![], goto(3)),
        block(
            vec![assign(
                2,
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_usize(2)))),
            )],
            return_term(),
        ),
    ]);

    let mut interp = Interpreter::new(&tcx);
    interp
        .run_body(&body)
        .expect("a dead `()`-temp read at a one-sided join must yield Unit, not panic");
}

/// The loop-desugar phantom: a `!`-typed temp moved on the continue edge of
/// `loop { … break … }` is never written anywhere — read yields `Unit`.
#[test]
fn uninit_never_temp_read_yields_unit() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let never_ty = tcx_mut.mk_ty(TyKind::Never);
    let tcx = tcx_mut.freeze();

    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![decl(i32_ty), decl(i32_ty), decl(never_ty)]);
    body.basic_blocks = IndexVec::from_vec(vec![
        block(vec![assign(1, Rvalue::Use(const_i32(0, i32_ty)))], goto(1)),
        block(
            vec![assign(
                1,
                Rvalue::BinaryOp(
                    BinOp::Add,
                    [
                        Operand::Copy(Place::new(LocalIdx::from_usize(1))),
                        const_i32(1, i32_ty),
                    ],
                ),
            )],
            Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_usize(1))),
                    switch_ty: i32_ty,
                    targets: glyim_mir::SwitchTargets::new(
                        vec![(3, BasicBlockIdx::from_usize(3))].into_boxed_slice(),
                        BasicBlockIdx::from_usize(2),
                    ),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ),
        block(
            vec![assign(
                2,
                Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_usize(2)))),
            )],
            goto(1),
        ),
        block(
            vec![assign(
                0,
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_usize(1)))),
            )],
            return_term(),
        ),
    ]);

    let mut interp = Interpreter::new(&tcx);
    let out = interp.run_body(&body);
    assert!(
        out.is_ok(),
        "the `!`-phantom move on the continue edge must yield Unit, not panic: {out:?}"
    );
    let counter = interp.get_local_value(LocalIdx::from_usize(1)).unwrap();
    assert_eq!(counter, &InterpValue::Int(3), "loop must run to the break");
}

/// The genuine guard is intact: an INHABITED-typed local read before any
/// write still panics with the precise "read from uninitialized" error.
#[test]
fn uninit_inhabited_read_still_panics() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let tcx = tcx_mut.freeze();

    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![decl(i32_ty), decl(i32_ty)]);
    body.basic_blocks = IndexVec::from_vec(vec![block(
        vec![assign(
            0,
            Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_usize(1)))),
        )],
        return_term(),
    )]);

    let mut interp = Interpreter::new(&tcx);
    let err = interp.run_body(&body).expect_err("must still panic");
    let msg = match &err {
        InterpError::Panic(m) => m.clone(),
        other => panic!("expected a panic error, got {other:?}"),
    };
    assert!(
        msg.contains("read from uninitialized local 1"),
        "guard message must name the local, got: {msg}"
    );
}
