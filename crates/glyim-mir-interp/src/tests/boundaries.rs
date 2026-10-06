use crate::*;
use glyim_core::{CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    Statement, StatementKind, SwitchTargets, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Const, ConstKind, Ty, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

// ============ Len rvalue with array type ============

#[test]
fn len_of_array_returns_correct_count() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = tcx_mut.mk_ty(TyKind::Array(
        i32_ty,
        Const {
            kind: ConstKind::Int(5),
            ty: Ty::BOOL,
        },
    ));
    let mut body = Body::dummy(dummy_def_id());
    let arr_local = LocalIdx::from_raw(1);
    let len_local = LocalIdx::from_raw(2);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(array_ty, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(Place::new(len_local), Rvalue::Len(Place::new(arr_local))),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(2)),
        Some(&InterpValue::Int(5))
    );
}

#[test]
fn len_of_non_array_panics() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let arr_local = LocalIdx::from_raw(1);
    let len_local = LocalIdx::from_raw(2);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(Place::new(len_local), Rvalue::Len(Place::new(arr_local))),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("expected array"));
}

// ============ Call with no target (fall through to next block) ============

#[test]
fn call_with_no_target_falls_through() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));

    // Callee: assigns 77 to return place and returns
    let callee_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let mut callee = Body::dummy(callee_id);
    callee.locals = IndexVec::from_raw(vec![local_decl(i32_ty, Mutability::Mut)]);
    callee.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(77),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);

    // Caller: BB0: call callee -> BB1: use result
    //
    // T108-PATCHED [INT-5]: a Call terminator that resumes at the following
    // block must use `target: Some(bb1)`. `target: None` is now reserved for
    // diverging calls (panic!/abort-style); the interpreter treats it as
    // "no resume point" and raises an InterpError if the callee ever
    // returns. This test exercises the normal-resume path, so it must set
    // the target explicitly.
    let mut caller = Body::dummy(dummy_def_id());
    let result_local = LocalIdx::from_raw(1);
    caller.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    let bb1 = BasicBlockIdx::from_raw(1);
    caller.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: Operand::Constant(MirConst {
                        kind: MirConstKind::Int(callee_id.local_id.to_raw() as i128),
                        ty: i32_ty,
                        span: Span::DUMMY,
                    }),
                    args: vec![],
                    destination: Place::new(result_local),
                    target: Some(bb1),
                    cleanup: None,
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

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_id, callee);
    interp.run_body(&caller).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(77))
    );
}

/// T108-REGRESSION: a Call with `target: None` is a diverging call. If the
/// callee returns anyway, the interpreter surfaces a clear error instead of
/// jumping to a random block index.
#[test]
fn call_with_no_target_diverges_cleanly_if_callee_returns() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));

    let callee_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let mut callee = Body::dummy(callee_id);
    callee.locals = IndexVec::from_raw(vec![local_decl(i32_ty, Mutability::Mut)]);
    callee.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);

    let mut caller = Body::dummy(dummy_def_id());
    caller.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    caller.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Call {
                func: Operand::Constant(MirConst {
                    kind: MirConstKind::Int(callee_id.local_id.to_raw() as i128),
                    ty: i32_ty,
                    span: Span::DUMMY,
                }),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(1)),
                target: None,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);

    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_id, callee);
    match interp.run_body(&caller) {
        Err(InterpError::Panic(msg)) => {
            assert!(
                msg.contains("diverging"),
                "expected 'diverging call returned' panic, got: {msg}"
            );
        }
        other => panic!("expected InterpError::Panic, got {other:?}"),
    }
}

// ============ Assert true + expected true (passes) ============

#[test]
fn assert_true_expected_true_passes() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Not),
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(true),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Assert {
                    cond: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    expected: true,
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                    msg: AssertMessage::DivisionByZero,
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
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
}

// ============ Assert non-bool condition panics ============

#[test]
fn assert_non_bool_condition_panics() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Not),
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: i32_ty,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Assert {
                cond: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                expected: true,
                target: BasicBlockIdx::from_raw(1),
                cleanup: None,
                msg: AssertMessage::BoundsCheck,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("bool"));
}

// ============ Empty body (just Return) ============

#[test]
fn empty_body_returns_immediately() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap(); // 1 step, should succeed
}

// ============ Step limit exactly at max steps ============

#[test]
fn step_limit_exactly_at_max_succeeds() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = {
        let mut b = Body::dummy(dummy_def_id());
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        // Build 3 blocks: BB0 -> goto BB1 -> goto BB2 -> Return
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(1),
                    },
                    source_info: SourceInfo::new(Span::DUMMY),
                },
                is_cleanup: false,
            },
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(2),
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
    let mut interp = Interpreter::new(&tcx).with_step_limit(3);
    interp.run_body(&body).unwrap();
}

#[test]
fn step_limit_one_short_fails() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = {
        let mut b = Body::dummy(dummy_def_id());
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Goto {
                        target: BasicBlockIdx::from_raw(1),
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
    let mut interp = Interpreter::new(&tcx).with_step_limit(1);
    let res = interp.run_body(&body);
    assert_eq!(res, Err(InterpError::TimedOut));
}

// ============ Recursion limit exactly at depth ============

#[test]
fn recursion_limit_exactly_one_succeeds() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let callee_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let callee = {
        let mut b = Body::dummy(callee_id);
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        b.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Unit,
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        }]);
        b
    };
    let caller = {
        let mut b = Body::dummy(dummy_def_id());
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Int(callee_id.local_id.to_raw() as i128),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(1)),
                        cleanup: None,
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
    let mut interp = Interpreter::new(&tcx).with_recursion_limit(2);
    interp.add_function(callee_id, callee);
    interp.run_body(&caller).unwrap();
}

#[test]
fn recursion_limit_exactly_one_less_fails() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let callee_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let callee = {
        let mut b = Body::dummy(callee_id);
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        b.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Unit,
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        }]);
        b
    };
    let caller = {
        let mut b = Body::dummy(dummy_def_id());
        b.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
        b.basic_blocks = IndexVec::from_raw(vec![
            BasicBlockData {
                statements: vec![],
                terminator: Terminator {
                    kind: TerminatorKind::Call {
                        func: Operand::Constant(MirConst {
                            kind: MirConstKind::Int(callee_id.local_id.to_raw() as i128),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        }),
                        args: vec![],
                        destination: Place::new(LocalIdx::from_raw(0)),
                        target: Some(BasicBlockIdx::from_raw(1)),
                        cleanup: None,
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
    let mut interp = Interpreter::new(&tcx).with_recursion_limit(1);
    interp.add_function(callee_id, callee);
    let res = interp.run_body(&caller);
    assert_eq!(res, Err(InterpError::StackOverflow));
}

// ============ SwitchInt with no matching branch ============

#[test]
fn switch_int_no_match_falls_to_otherwise() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let discr_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Not),
    ]);
    let assign = Statement {
        kind: StatementKind::Assign(
            Place::new(discr_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(999),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let targets = SwitchTargets::new(
        vec![(1u128, BasicBlockIdx::from_raw(1))].into_boxed_slice(),
        BasicBlockIdx::from_raw(2), // otherwise
    );
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![assign],
            terminator: Terminator {
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(discr_local)),
                    switch_ty: i32_ty,
                    targets,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
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
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap(); // goes to otherwise → Return
}
