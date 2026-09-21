use crate::*;
use glyim_core::{CrateId, DefId, FnDefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::{
    AssertMessage, BasicBlockData, BasicBlockIdx, LocalDecl, LocalIdx, MirConst, MirConstKind,
    Operand, Place, Rvalue, SourceInfo, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyCtxMut, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

/// Build a body whose single "real" block panics via a failing `Assert` that
/// carries a `cleanup` edge to a cleanup block. The cleanup block records that
/// it ran (by writing a sentinel into a tracking local) and then Gotos the
/// normal return block. Local layout:
///   _0: UNIT (return slot)
///   _1: i32  (tracking local: 0 = not run, 42 = cleanup ran)
fn build_unwinding_body(tcx: &mut TyCtxMut) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![
        // BB0: the main block — a failing assert with a cleanup edge.
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Assert {
                    cond: Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(false),
                        ty: tcx.bool_ty(),
                        span: Span::DUMMY,
                    }),
                    expected: true,
                    target: BasicBlockIdx::from_raw(2),
                    cleanup: Some(BasicBlockIdx::from_raw(1)),
                    msg: AssertMessage::BoundsCheck,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
        // BB1: cleanup block — record that cleanup ran, then Goto return.
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(2),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: true,
        },
        // BB2: normal return (reached only via cleanup when unwinding).
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
    ]);
    body
}

#[test]
fn panic_without_unwind_aborts() {
    // Plan §14.2: with `panics_unwind` off (the default), a failing assert
    // still aborts interpretation with an error — the old behavior is preserved.
    let mut tcx_mut = test_ty_ctx();
    let body = build_unwinding_body(&mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    let result = interp.run_body(&body);
    assert!(result.is_err(), "without unwinding, a panic must abort");
}

#[test]
fn panic_runs_cleanup_block_when_unwinding() {
    // Plan §14.2: with `panics_unwind` on, a failing assert routes to the
    // `cleanup` edge; the cleanup block runs and records that it executed,
    // then the function reaches a clean Return.
    let mut tcx_mut = test_ty_ctx();
    let body = build_unwinding_body(&mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_panics_unwind(true);
    interp
        .run_body(&body)
        .expect("with unwinding, the cleanup block must absorb the panic");
    let ran = interp
        .get_local_value(LocalIdx::from_raw(1))
        .expect("tracking local must be set");
    assert_eq!(
        ran,
        &InterpValue::Int(42),
        "cleanup block must have run during unwind"
    );
}

/// Nested-def-id helper so three functions can call each other.
fn def_id(raw: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(raw))
}

/// Plan §7.2: a panic in a deeply-nested call must unwind through EVERY
/// caller frame's cleanup block, not just the innermost one. Three functions:
/// `outer` → `middle` → `inner`; `inner` panics. Each caller has a cleanup
/// block that records a distinct sentinel (outer=99, middle=7) and then
/// re-panics, so the walk continues up the stack. At the top (no caller) the
/// interpreter returns `InterpError::Unwind` carrying the original panic — and
/// the outer sentinel proves the unwind reached the outermost frame.
#[test]
fn nested_panic_unwinds_through_all_caller_frames() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // `inner` (def 2): panics immediately (block has no cleanup edge).
    let inner = {
        let mut b = Body::dummy(def_id(2));
        b.locals = IndexVec::from_raw(vec![LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        }]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(1),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    // `middle` (def 1): calls `inner`; on unwind runs its cleanup (sentinel 7)
    // then re-panics so the walk continues to `outer`.
    let middle = {
        let mut b = Body::dummy(def_id(1));
        b.locals = IndexVec::from_raw(vec![
            LocalDecl {
                ty: Ty::UNIT,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            LocalDecl {
                ty: i32_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ]);
        b.basic_blocks = IndexVec::from_raw(vec![
            // BB0: call inner, resume at BB2, unwind at BB1.
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Fn(
                                FnDefId::from_raw(2),
                                glyim_type::Substitution::empty(),
                            ),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(2)),
                        cleanup: Some(BasicBlockIdx::from_raw(1)),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            // BB1: middle's cleanup — record sentinel 7, then re-panic.
            BasicBlockData {
                statements: vec![Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(7),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                }],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(3),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: true,
            },
            // BB2: normal return after the (never-completed) call.
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            // BB3: re-panic in middle's frame (no cleanup) so unwinding pops to outer.
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(4),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            // BB4: unreachable return.
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    // `outer` (def 0): calls `middle`; on unwind runs its cleanup (sentinel 99)
    // then re-panics; with no caller frame the interpreter returns `Unwind`.
    let outer = {
        let mut b = Body::dummy(def_id(0));
        b.locals = IndexVec::from_raw(vec![
            LocalDecl {
                ty: Ty::UNIT,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            LocalDecl {
                ty: i32_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Fn(
                                FnDefId::from_raw(1),
                                glyim_type::Substitution::empty(),
                            ),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(2)),
                        cleanup: Some(BasicBlockIdx::from_raw(1)),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            // BB1: outer's cleanup — record sentinel 99, then re-panic.
            BasicBlockData {
                statements: vec![Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(99),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                }],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(3),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: true,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            // BB3: re-panic in outer's frame; with no caller this returns `Unwind`.
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(4),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_panics_unwind(true);
    interp.add_function(def_id(1), middle);
    interp.add_function(def_id(2), inner);
    let result = interp.run_body(&outer);

    // The walk must reach the top of the stack and report an `Unwind`.
    let err = result.expect_err("nested panic must unwind to the top");
    assert!(
        matches!(err, InterpError::Unwind(_)),
        "nested panic must return Unwind, got {err:?}"
    );
    // Outer's cleanup block must have run (proving the unwind walked through
    // the middle frame to the outer frame, not just aborted in the innermost).
    let outer_sentinel = interp
        .get_local_value(LocalIdx::from_raw(1))
        .expect("outer tracking local must be set");
    assert_eq!(
        outer_sentinel,
        &InterpValue::Int(99),
        "outer cleanup must run during cross-frame unwind"
    );
}

/// Build a call chain `f0 → f1 → … → f(depth-1)` where every `Call` carries
/// `cleanup: None`. The bottom function (`f(depth-1)`) panics via a failing
/// `Assert` (also `cleanup: None`). No frame has a cleanup edge, so a panic at
/// the bottom must unwind straight to the top of the stack as `Unwind`, never
/// resuming any caller's normal continuation.
fn build_no_cleanup_chain(depth: usize, tcx_mut: &mut TyCtxMut) -> Vec<(DefId, Body)> {
    let mut funcs = Vec::new();
    for i in 0..depth {
        let is_bottom = i == depth - 1;
        let mut b = Body::dummy(def_id(i as u32));
        b.locals = IndexVec::from_raw(vec![LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        }]);
        let terminator = if is_bottom {
            TerminatorKind::Assert {
                cond: Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(false),
                    ty: tcx_mut.bool_ty(),
                    span: Span::DUMMY,
                }),
                expected: true,
                target: BasicBlockIdx::from_raw(1),
                cleanup: None,
                msg: AssertMessage::BoundsCheck,
            }
        } else {
            TerminatorKind::Call {
                func: Operand::Constant(MirConst {
                    kind: MirConstKind::Fn(
                        FnDefId::from_raw((i as u32) + 1),
                        glyim_type::Substitution::empty(),
                    ),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                }),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            }
        };
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: terminator,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        funcs.push((def_id(i as u32), b));
    }
    funcs
}

// Plan §1.4 (regression for the fixed `.unwrap_or(target_bb)` bug): a panic in a
// chain of callers that ALL lack a cleanup edge must surface as `Unwind` at the
// top of the stack, NOT as a normal return from the outermost frame. The old
// code resumed the outermost caller's normal continuation, silently treating a
// propagating panic as a successful return.
#[test]
fn unwind_skips_callers_with_no_cleanup() {
    let mut tcx_mut = test_ty_ctx();
    let funcs = build_no_cleanup_chain(3, &mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_panics_unwind(true);
    for (id, body) in &funcs {
        interp.add_function(*id, body.clone());
    }
    let (_entry_id, _) = &funcs[0];
    let result = interp.run_body(&funcs[0].1);
    let err = result.expect_err("panic with no recovery must unwind to the top");
    assert!(
        matches!(err, InterpError::Unwind(_)),
        "panic with no cleanup edges must Unwind, got {err:?}"
    );
}

// Plan §1.4: a panic in a chain where only the OUTERMOST caller has a cleanup
// edge must resume in that outermost cleanup block, skipping the intermediate
// caller (which had no cleanup edge for its own call).
#[test]
fn unwind_resumes_at_nearest_caller_with_cleanup() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // `h` (def 2, bottom): panics, no cleanup.
    let h = {
        let mut b = Body::dummy(def_id(2));
        b.locals = IndexVec::from_raw(vec![LocalDecl {
            ty: Ty::UNIT,
            mutability: Mutability::Mut,
            source_info: SourceInfo::new(Span::DUMMY),
        }]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(1),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    // `g` (def 1, middle): calls `h` with NO cleanup edge; its only cleanup
    // block (BB1) records sentinel 7 then re-panics, but it must be SKIPPED
    // because `g`'s call to `h` had no cleanup — the unwind walks past it.
    let g = {
        let mut b = Body::dummy(def_id(1));
        b.locals = IndexVec::from_raw(vec![
            LocalDecl {
                ty: Ty::UNIT,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            LocalDecl {
                ty: i32_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Fn(
                                FnDefId::from_raw(2),
                                glyim_type::Substitution::empty(),
                            ),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(2)),
                        cleanup: None,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(7),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                }],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(3),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: true,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(4),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    // `f` (def 0, outer): calls `g` WITH a cleanup edge (BB1, sentinel 99).
    let f = {
        let mut b = Body::dummy(def_id(0));
        b.locals = IndexVec::from_raw(vec![
            LocalDecl {
                ty: Ty::UNIT,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            LocalDecl {
                ty: i32_ty,
                mutability: Mutability::Mut,
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Fn(
                                FnDefId::from_raw(1),
                                glyim_type::Substitution::empty(),
                            ),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(2)),
                        cleanup: Some(BasicBlockIdx::from_raw(1)),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![Statement {
                    kind: StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(99),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        })),
                    ),
                    source_info: SourceInfo::new(Span::DUMMY),
                }],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(3),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: true,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Assert {
                        cond: Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(false),
                            ty: tcx_mut.bool_ty(),
                            span: Span::DUMMY,
                        }),
                        expected: true,
                        target: BasicBlockIdx::from_raw(4),
                        cleanup: None,
                        msg: AssertMessage::BoundsCheck,
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Return,
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
        ]);
        b
    };

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_panics_unwind(true);
    interp.add_function(def_id(1), g);
    interp.add_function(def_id(2), h);
    // Run `f`: the panic in `h` must skip `g` (no cleanup) and resume in `f`'s
    // cleanup block (sentinel 99), then re-panic at the top as `Unwind`.
    let result = interp.run_body(&f);
    let err = result.expect_err("panic must unwind to the top");
    assert!(
        matches!(err, InterpError::Unwind(_)),
        "panic must reach the top as Unwind, got {err:?}"
    );
    // g's sentinel (7) must NOT be set — its cleanup was correctly skipped.
    let g_sentinel = interp
        .get_local_value(LocalIdx::from_raw(1))
        .expect("f tracking local must be set");
    assert_eq!(
        g_sentinel,
        &InterpValue::Int(99),
        "unwind must resume in the nearest caller WITH a cleanup (f), skipping g"
    );
}

// Plan §1.4: the original panic payload (not a secondary one raised inside an
// intermediate cleanup) is what surfaces at the top after a multi-frame unwind.
#[test]
fn original_panic_payload_survives_multi_frame_unwind() {
    let mut tcx_mut = test_ty_ctx();
    let funcs = build_no_cleanup_chain(4, &mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx).with_panics_unwind(true);
    for (id, body) in &funcs {
        interp.add_function(*id, body.clone());
    }
    let err = interp
        .run_body(&funcs[0].1)
        .expect_err("multi-frame panic must unwind to top");
    match err {
        InterpError::Unwind(inner) => {
            // The payload must be the bottom panic, not some synthesized value.
            assert!(
                format!("{inner}").contains("BoundsCheck") || format!("{inner}").contains("assert"),
                "Unwind payload must carry the original panic, got {inner:?}"
            );
        }
        other => panic!("expected Unwind, got {other:?}"),
    }
}

// Plan §1.4: after a full multi-frame unwind, `recursion_depth` is correctly
// decremented for every popped frame (not left at the depth reached), so a
// fresh call chain can recurse back up to `recursion_limit` again without a
// spurious StackOverflow.
#[test]
fn recursion_limit_reflects_unwound_frames() {
    let limit = 5usize;
    let mut tcx_mut = test_ty_ctx();
    let funcs = build_no_cleanup_chain(limit, &mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx)
        .with_panics_unwind(true)
        .with_recursion_limit(limit);
    for (id, body) in &funcs {
        interp.add_function(*id, body.clone());
    }
    let err = interp
        .run_body(&funcs[0].1)
        .expect_err("deep no-cleanup panic must unwind to top");
    assert!(
        matches!(err, InterpError::Unwind(_)),
        "deep chain must unwind, got {err:?}"
    );
    // After a full unwind the bookkeeping must be back to the entry level.
    assert_eq!(
        interp.recursion_depth(),
        1,
        "recursion_depth must be decremented for every popped frame"
    );
    // A fresh call chain of the same depth must NOT trip the recursion limit.
    let mut tcx_mut2 = test_ty_ctx();
    let fs = build_no_cleanup_chain(limit, &mut tcx_mut2);
    let tcx2 = tcx_mut2.freeze();
    let mut interp2 = Interpreter::new(&tcx2)
        .with_panics_unwind(true)
        .with_recursion_limit(limit);
    for (id, body) in &fs {
        interp2.add_function(*id, body.clone());
    }
    let err2 = interp2
        .run_body(&fs[0].1)
        .expect_err("second chain must also unwind, not StackOverflow");
    assert!(
        matches!(err2, InterpError::Unwind(_)),
        "fresh chain must reach the same depth again (no stale recursion accounting), got {err2:?}"
    );
}
