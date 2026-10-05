use crate::*;
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, LocalIdx, MirConst, MirConstKind, Operand, Place, Rvalue,
    Statement, StatementKind, SwitchTargets, Terminator, TerminatorKind,
};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::{Ty, TyCtxMut, TyKind};

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

// ============ Bool And/Or ============

fn build_bool_binop_body(op: BinOp, left: bool, right: bool) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    let bool_ty = Ty::BOOL;
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Mut),
    ]);
    let c1 = MirConst {
        kind: MirConstKind::Bool(left),
        ty: bool_ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Bool(right),
        ty: bool_ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res_local),
                Rvalue::BinaryOp(op, Box::new((Operand::Constant(c1), Operand::Constant(c2)))),
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

// ============ Nested SwitchInt (3-way) ============
fn build_three_way_switch_body(tcx: &mut TyCtxMut, val: i128) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let discr_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Not),
    ]);
    let assign_stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(discr_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(val),
                ty: i32_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let then_target = BasicBlockIdx::from_raw(1);
    let else_target = BasicBlockIdx::from_raw(2);
    let otherwise_target = BasicBlockIdx::from_raw(3);
    let targets = SwitchTargets::new(
        vec![(1u128, then_target), (2u128, else_target)].into_boxed_slice(),
        otherwise_target,
    );
    let switch = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(discr_local)),
            switch_ty: i32_ty,
            targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    // Each target assigns a marker value to local 1 and returns
    fn marker_block(_bb_idx: u32, val: i128, ty: Ty) -> BasicBlockData {
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(val),
                        ty,
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
        }
    }
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![assign_stmt],
            terminator: switch,
            is_cleanup: false,
        },
        marker_block(1, 100, i32_ty),
        marker_block(2, 200, i32_ty),
        marker_block(3, 300, i32_ty),
    ]);
    body
}

// ============ Call with arguments ============
fn build_callee_with_args_body(tcx: &mut TyCtxMut) -> Body {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)));
    let ret_local = LocalIdx::from_raw(0);
    let arg_local = LocalIdx::from_raw(1);
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    body.locals = IndexVec::from_raw(vec![
        local_decl(i32_ty, Mutability::Mut), // return
        local_decl(i32_ty, Mutability::Not), // arg0
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(ret_local),
                Rvalue::BinaryOp(
                    BinOp::Mul,
                    Box::new((
                        Operand::Copy(Place::new(arg_local)),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(2),
                            ty: i32_ty,
                            span: Span::DUMMY,
                        }),
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

fn build_caller_with_args_body(tcx: &mut TyCtxMut, callee_def_id: DefId, arg_val: i128) -> Body {
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let ret_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    let fn_const = Operand::Constant(MirConst {
        kind: MirConstKind::Int(callee_def_id.local_id.to_raw() as i128),
        ty: i32_ty,
        span: Span::DUMMY,
    });
    let arg_const = Operand::Constant(MirConst {
        kind: MirConstKind::Int(arg_val),
        ty: i32_ty,
        span: Span::DUMMY,
    });
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: fn_const,
                    args: vec![arg_const],
                    destination: Place::new(ret_local),
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
    body
}

// ============ Nop statement ============
fn build_nop_body() -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Nop,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Nop,
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    body
}

// ============ Step limit exact ============
fn build_two_step_body() -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![local_decl(Ty::UNIT, Mutability::Mut)]);
    body.basic_blocks = IndexVec::from_raw(vec![
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
    body
}

// ===================== TESTS =====================

#[test]
fn bool_and_true() {
    let body = build_bool_binop_body(BinOp::And, true, true);
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn bool_and_false() {
    let body = build_bool_binop_body(BinOp::And, true, false);
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(false))
    );
}

#[test]
fn bool_or() {
    let body = build_bool_binop_body(BinOp::Or, false, true);
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn bool_or_false() {
    let body = build_bool_binop_body(BinOp::Or, false, false);
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(false))
    );
}

#[test]
fn nested_switch_first_case() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_three_way_switch_body(&mut tcx_mut, 1);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(100))
    );
}

#[test]
fn nested_switch_second_case() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_three_way_switch_body(&mut tcx_mut, 2);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(200))
    );
}

#[test]
fn nested_switch_otherwise() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_three_way_switch_body(&mut tcx_mut, 99);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(300))
    );
}

#[test]
fn call_with_args_and_return() {
    let mut tcx_mut = test_ty_ctx();
    let callee_body = build_callee_with_args_body(&mut tcx_mut);
    let callee_id = callee_body.owner;
    let caller_body = build_caller_with_args_body(&mut tcx_mut, callee_id, 21);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_id, callee_body);
    interp.run_body(&caller_body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(42))
    );
}

#[test]
fn nop_statements_run_ok() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_nop_body();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
}

#[test]
fn step_limit_exact_should_pass() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_two_step_body();
    let mut interp = Interpreter::new(&tcx).with_step_limit(2);
    interp.run_body(&body).unwrap();
}

#[test]
fn step_limit_exact_plus_one_fails() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_two_step_body();
    let mut interp = Interpreter::new(&tcx).with_step_limit(1);
    let res = interp.run_body(&body);
    assert_eq!(res, Err(InterpError::TimedOut));
}

// ============ Bool Not (Unary) ============

#[test]
fn bool_not_true() {
    use glyim_core::UnOp;
    let tcx_mut = test_ty_ctx();
    let bool_ty = Ty::BOOL;
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Bool(true),
        ty: bool_ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res_local),
                Rvalue::UnaryOp(UnOp::Not, Operand::Constant(c)),
            ),
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
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(false))
    );
}

#[test]
fn bool_not_false() {
    use glyim_core::UnOp;
    let tcx_mut = test_ty_ctx();
    let bool_ty = Ty::BOOL;
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Bool(false),
        ty: bool_ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res_local),
                Rvalue::UnaryOp(UnOp::Not, Operand::Constant(c)),
            ),
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
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

// ============ Uint constant ============

#[test]

fn uint_constant_interpreted_as_int() {
    let mut tcx_mut = test_ty_ctx();
    let u32_ty = tcx_mut.mk_ty(TyKind::Uint(glyim_core::UintTy::U32));
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(u32_ty, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Uint(42),
        ty: u32_ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(Place::new(res_local), Rvalue::Use(Operand::Constant(c))),
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
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Uint(42))
    );
}

// ============ Unit value ============

#[test]
fn unit_constant() {
    let tcx_mut = test_ty_ctx();
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::UNIT, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Unit,
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(Place::new(res_local), Rvalue::Use(Operand::Constant(c))),
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
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Unit)
    );
}

// ============ Drop terminator ============

#[test]
// T009-PATCHED-TEST-EDGE [INT-2]: the previous test asserted a panic when
// a `Drop` terminator reached the interpreter. That invariant was based on
// the false premise that drop elaboration rewrites every `Drop` into a
// `Call`; it does not — `elaborate_drops` keeps the `Drop` terminator and
// only guards it with a drop flag. The interpreter now treats the
// terminator as a no-op and proceeds to the target until real per-type
// drop glue lands (see LL-15). The test therefore verifies clean
// execution, not a panic.
fn drop_terminator_proceeds_to_target() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Mut),
    ]);
    let local_to_drop = LocalIdx::from_raw(1);
    // BB0: assign true to local 1, then Drop(local 1) -> BB1
    // BB1: Return
    // A bare `Drop` reaching the interpreter is a no-op until real drop
    // glue is wired end-to-end.
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(local_to_drop),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(true),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Drop {
                    place: Place::new(local_to_drop),
                    target: BasicBlockIdx::from_raw(1),
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
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap(); // should reach Return via Drop
}

// ============ Sequential binary ops ============

#[test]
fn sequential_binary_ops() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let loc_a = LocalIdx::from_raw(1);
    let loc_b = LocalIdx::from_raw(2);
    let loc_result = LocalIdx::from_raw(3);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
        local_decl(i32_ty, Mutability::Mut),
    ]);
    let c10 = MirConst {
        kind: MirConstKind::Int(10),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let c3 = MirConst {
        kind: MirConstKind::Int(3),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Int(2),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![
            // a = 10 + 3 = 13
            Statement {
                kind: StatementKind::Assign(
                    Place::new(loc_a),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((Operand::Constant(c10), Operand::Constant(c3))),
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            // b = a * 2 = 26
            Statement {
                kind: StatementKind::Assign(
                    Place::new(loc_b),
                    Rvalue::BinaryOp(
                        BinOp::Mul,
                        Box::new((Operand::Copy(Place::new(loc_a)), Operand::Constant(c2))),
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            // result = b - 6 = 20
            Statement {
                kind: StatementKind::Assign(
                    Place::new(loc_result),
                    Rvalue::BinaryOp(
                        BinOp::Sub,
                        Box::new((
                            Operand::Copy(Place::new(loc_b)),
                            Operand::Constant(MirConst {
                                kind: MirConstKind::Int(6),
                                ty: i32_ty,
                                span: Span::DUMMY,
                            }),
                        )),
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ],
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
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(13))
    );
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(2)),
        Some(&InterpValue::Int(26))
    );
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(3)),
        Some(&InterpValue::Int(20))
    );
}

// ============ get_local_value on uninitialized ============

#[test]
fn get_local_value_none_for_uninitialized() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Mut),
    ]);
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(interp.get_local_value(LocalIdx::from_raw(1)), None);
}

// ============ Step limit 0 ============

#[test]
fn step_limit_zero_always_timed_out() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_two_step_body();
    let mut interp = Interpreter::new(&tcx).with_step_limit(0);
    let res = interp.run_body(&body);
    assert_eq!(res, Err(InterpError::TimedOut));
}
