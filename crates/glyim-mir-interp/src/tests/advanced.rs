use crate::*;
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
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

// ----- Helper: build body with a single binary op -----
fn build_binop_body(tcx: &mut TyCtxMut, op: BinOp, lhs: i128, rhs: i128) -> Body {
    let ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c1 = MirConst {
        kind: MirConstKind::Int(lhs),
        ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Int(rhs),
        ty,
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

// ----- Helper: build body with unary op -----
fn build_unary_body(tcx: &mut TyCtxMut, op: UnOp, operand: i128) -> Body {
    let ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(ty, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Int(operand),
        ty,
        span: Span::DUMMY,
    };
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(res_local),
                Rvalue::UnaryOp(op, Operand::Constant(c)),
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

// ----- Helper: build body with comparison returning bool -----
fn build_cmp_body(tcx: &mut TyCtxMut, op: BinOp, lhs: i128, rhs: i128) -> Body {
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = Ty::BOOL;
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Mut),
    ]);
    let c1 = MirConst {
        kind: MirConstKind::Int(lhs),
        ty: int_ty,
        span: Span::DUMMY,
    };
    let c2 = MirConst {
        kind: MirConstKind::Int(rhs),
        ty: int_ty,
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

// ----- Helper: build multi-block body with Goto chain -----
fn build_goto_chain_body(final_val: i128) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let res_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Mut),
    ]);
    let c = MirConst {
        kind: MirConstKind::Int(final_val),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    };
    // BB0: goto BB1
    // BB1: goto BB2
    // BB2: assign val; return
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
                kind: TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(2),
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(res_local),
                    Rvalue::Use(Operand::Constant(c)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
    ]);
    body
}

// ----- Helper: build body with Assert terminator -----
fn build_assert_body(expected: bool, cond_val: bool) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    let bool_ty = Ty::BOOL;
    let cond_local = LocalIdx::from_raw(1);
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(bool_ty, Mutability::Not),
    ]);
    let assign = Statement {
        kind: StatementKind::Assign(
            Place::new(cond_local),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Bool(cond_val),
                ty: bool_ty,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![assign],
            terminator: Terminator {
                kind: TerminatorKind::Assert {
                    cond: Operand::Copy(Place::new(cond_local)),
                    expected,
                    target: BasicBlockIdx::from_raw(1),
                    cleanup: None,
                    msg: AssertMessage::Overflow(BinOp::Add),
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

// ----- Helper: build body reading uninitialized local -----
fn build_uninit_read_body() -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_raw(vec![
        local_decl(Ty::UNIT, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Mut),
        local_decl(Ty::BOOL, Mutability::Mut),
    ]);
    // Read from local 2 (uninitialized), store to local 1
    body.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(2)))),
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

// ----- Helper: build body with nested function calls -----
fn build_nested_call_bodies(tcx: &mut TyCtxMut) -> (Body, Body, Body, DefId, DefId) {
    // deepest: returns 100
    let deepest_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1));
    let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let mut deepest = Body::dummy(deepest_id);
    deepest.locals = IndexVec::from_raw(vec![local_decl(i32_ty, Mutability::Mut)]);
    let c100 = MirConst {
        kind: MirConstKind::Int(100),
        ty: i32_ty,
        span: Span::DUMMY,
    };
    deepest.basic_blocks = IndexVec::from_raw(vec![BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(c100)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }]);

    // middle: calls deepest, adds 50
    let middle_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(2));
    let mut middle = Body::dummy(middle_id);
    middle.locals = IndexVec::from_raw(vec![
        local_decl(i32_ty, Mutability::Mut), // return place
        local_decl(i32_ty, Mutability::Mut), // temp for call result
    ]);
    // Encode deepest_id as an Int constant for the call
    let fn_const = Operand::Constant(MirConst {
        kind: MirConstKind::Int(1), // LocalDefId raw
        ty: i32_ty,
        span: Span::DUMMY,
    });
    middle.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: fn_const,
                    args: vec![],
                    destination: Place::new(LocalIdx::from_raw(1)),
                    target: Some(BasicBlockIdx::from_raw(1)),
                    cleanup: None,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        },
        BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            Operand::Constant(MirConst {
                                kind: MirConstKind::Int(50),
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
        },
    ]);

    // top: calls middle, returns result
    let top_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(3));
    let mut top = Body::dummy(top_id);
    top.locals = IndexVec::from_raw(vec![local_decl(i32_ty, Mutability::Mut)]);
    let fn_const2 = Operand::Constant(MirConst {
        kind: MirConstKind::Int(2), // middle LocalDefId raw
        ty: i32_ty,
        span: Span::DUMMY,
    });
    top.basic_blocks = IndexVec::from_raw(vec![
        BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: fn_const2,
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

    (deepest, middle, top, deepest_id, middle_id)
}

// ============================================================
// Tests
// ============================================================

#[test]
fn interpret_multiplication() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Mul, 6, 7);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(42))
    );
}

#[test]
fn interpret_division() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Div, 42, 6);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(7))
    );
}

#[test]
fn interpret_division_by_zero_panics() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Div, 42, 0);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(matches!(res, Err(InterpError::DivisionByZero)));
}

#[test]
fn interpret_remainder() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Rem, 17, 5);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(2))
    );
}

#[test]
fn interpret_remainder_by_zero_panics() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Rem, 42, 0);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(matches!(res, Err(InterpError::DivisionByZero)));
}

#[test]
fn interpret_bitwise_and() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::BitAnd, 0b1100, 0b1010);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(0b1000))
    );
}

#[test]
fn interpret_bitwise_or() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::BitOr, 0b1100, 0b1010);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(0b1110))
    );
}

#[test]
fn interpret_bitwise_xor() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::BitXor, 0b1100, 0b1010);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(0b0110))
    );
}

#[test]
fn interpret_shift_left() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Shl, 1, 4);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(16))
    );
}

#[test]
fn interpret_shift_right() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_binop_body(&mut tcx_mut, BinOp::Shr, 16, 2);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(4))
    );
}

#[test]
fn interpret_eq_true() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Eq, 5, 5);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_eq_false() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Eq, 5, 7);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(false))
    );
}

#[test]
fn interpret_ne() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Ne, 5, 7);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_lt_true() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Lt, 3, 7);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_lt_false() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Lt, 7, 3);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(false))
    );
}

#[test]
fn interpret_gt() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::Gt, 7, 3);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_lteq() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::LtEq, 5, 5);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_gteq() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_cmp_body(&mut tcx_mut, BinOp::GtEq, 5, 5);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Bool(true))
    );
}

#[test]
fn interpret_unary_negate() {
    let mut tcx_mut = test_ty_ctx();
    let body = build_unary_body(&mut tcx_mut, UnOp::Neg, 42);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(-42))
    );
}

#[test]
fn interpret_goto_chain() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_goto_chain_body(99);
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(99))
    );
}

#[test]
fn interpret_assert_success() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_assert_body(true, true);
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap(); // should reach Return
}

#[test]
fn interpret_assert_failure_panics() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_assert_body(true, false);
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(matches!(res, Err(InterpError::Panic(ref msg)) if msg.contains("assert failed")));
}

#[test]
fn interpret_read_uninitialized_panics() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let body = build_uninit_read_body();
    let mut interp = Interpreter::new(&tcx);
    let res = interp.run_body(&body);
    assert!(matches!(res, Err(InterpError::Panic(ref msg)) if msg.contains("uninitialized")));
}

#[test]
fn interpret_nested_function_calls() {
    let mut tcx_mut = test_ty_ctx();
    let (deepest, middle, top, deepest_id, middle_id) = build_nested_call_bodies(&mut tcx_mut);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(deepest_id, deepest);
    interp.add_function(middle_id, middle);
    interp.run_body(&top).unwrap();
    // deepest returns 100, middle adds 50 = 150
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(0)),
        Some(&InterpValue::Int(150))
    );
}

#[test]
fn interpret_custom_step_limit() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let interp = Interpreter::new(&tcx).with_step_limit(500);
    assert_eq!(interp.step_limit(), 500);
}

#[test]
fn interpret_custom_recursion_limit() {
    let tcx = glyim_test::test_frozen_ty_ctx();
    let interp = Interpreter::new(&tcx).with_recursion_limit(128);
    assert_eq!(interp.recursion_limit(), 128);
}

#[test]
fn interpret_wrapping_add() {
    let mut tcx_mut = test_ty_ctx();
    // The body's result local is `i32`, so wrapping is at 32-bit width
    // (MIR-13): `i32::MAX + 1` wraps to `i32::MIN`.
    let body = build_binop_body(&mut tcx_mut, BinOp::Add, i32::MAX as i128, 1);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int(i32::MIN as i128))
    );
}

#[test]
fn interpret_wrapping_mul() {
    let mut tcx_mut = test_ty_ctx();
    // 32-bit wrapping: `i32::MAX * 2` wraps to `-2` (MIR-13).
    let body = build_binop_body(&mut tcx_mut, BinOp::Mul, i32::MAX as i128, 2);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    assert_eq!(
        interp.get_local_value(LocalIdx::from_raw(1)),
        Some(&InterpValue::Int((i32::MAX as i128).wrapping_mul(2) as i32 as i128))
    );
}
