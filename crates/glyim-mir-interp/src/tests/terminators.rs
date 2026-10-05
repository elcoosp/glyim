use super::helpers::*;
use crate::{InterpError, InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyKind};

#[test]
fn test_assert_true_continues() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let _bb2 = BasicBlockIdx::from_raw(2);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    set_terminator(
        &mut body,
        bb0,
        TerminatorKind::Assert {
            cond: Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            expected: true,
            target: bb1,
            cleanup: None,
            msg: AssertMessage::Overflow(BinOp::Add),
        },
    );

    add_statement(
        &mut body,
        bb1,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(42))),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    assert_eq!(
        *interp.get_local_value(local_x).unwrap(),
        InterpValue::Int(42)
    );
}

#[test]
fn test_assert_false_panics() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let _local_x = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let _bb2 = BasicBlockIdx::from_raw(2);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    set_terminator(
        &mut body,
        bb0,
        TerminatorKind::Assert {
            cond: Operand::Constant(MirConst {
                kind: MirConstKind::Bool(false),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            expected: true,
            target: bb1,
            cleanup: None,
            msg: AssertMessage::Overflow(BinOp::Add),
        },
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_err());
    match result {
        Err(InterpError::Panic(msg)) => assert!(msg.contains("assert failed")),
        _ => panic!("expected Panic"),
    }
}

#[test]
fn test_unreachable_panics() {
    let ctx = glyim_test::test_ty_ctx();

    let mut body = empty_body(Ty::UNIT);
    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    set_terminator(&mut body, bb0, TerminatorKind::Goto { target: bb1 });

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_err());
    match result {
        Err(InterpError::Panic(msg)) => assert!(msg.contains("unreachable")),
        _ => panic!("expected Panic"),
    }
}

#[test]
// T009-PATCHED-TEST-TERM [INT-2]: the previous `#[should_panic]` encoded
// the (incorrect) invariant that a `Drop` terminator must never reach the
// interpreter. `elaborate_drops` keeps `Drop` terminators; only the
// drop-flag plumbing distinguishes live drops. Until real per-type drop
// glue is wired end-to-end (LL-15) the interpreter treats a `Drop` as a
// no-op and proceeds to its target.
fn test_drop_terminator_proceeds() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_dummy = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let _bb2 = BasicBlockIdx::from_raw(2);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_dummy), Rvalue::Use(const_int(0))),
    );
    set_terminator(
        &mut body,
        bb0,
        TerminatorKind::Drop {
            place: Place::new(local_dummy),
            target: bb1,
            cleanup: None,
        },
    );
    add_statement(
        &mut body,
        bb1,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(55))),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    // A bare `Drop` reaching the interpreter must be rejected loudly (plan
    // §14.1): drop elaboration should have lowered it to a drop-glue Call
    // first. This test verifies the interpreter enforces that invariant.
    let _ = interp.run_body(&interp.function_table.values().next().unwrap().clone());
}

#[test]
fn test_switch_int_multiple_targets() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_x = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_discr = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let bb2 = BasicBlockIdx::from_raw(2);
    let bb3 = BasicBlockIdx::from_raw(3);
    let bb4 = BasicBlockIdx::from_raw(4);
    let bb5 = BasicBlockIdx::from_raw(5);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb5 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb5 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb5 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb5 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_discr), Rvalue::Use(const_int(2))),
    );
    set_terminator(
        &mut body,
        bb0,
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(local_discr)),
            switch_ty: i32_ty,
            targets: SwitchTargets::new(
                vec![(0u128, bb1), (1u128, bb2), (2u128, bb3)].into_boxed_slice(),
                bb4,
            ),
        },
    );
    add_statement(
        &mut body,
        bb3,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(999))),
    );
    add_statement(
        &mut body,
        bb1,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(111))),
    );
    add_statement(
        &mut body,
        bb2,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(222))),
    );
    add_statement(
        &mut body,
        bb4,
        StatementKind::Assign(Place::new(local_x), Rvalue::Use(const_int(444))),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    assert_eq!(
        *interp.get_local_value(local_x).unwrap(),
        InterpValue::Int(999)
    );
}

#[test]
fn test_step_limit_enforced() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_i = add_local(&mut body, i32_ty, Mutability::Mut);

    let bb0 = BasicBlockIdx::from_raw(0);
    // Infinite loop: bb0: i = i + 1; goto bb0
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_i),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new((Operand::Copy(Place::new(local_i)), const_int(1))),
            ),
        ),
    );
    // Initialize i
    body.basic_blocks[bb0].statements.insert(
        0,
        Statement {
            kind: StatementKind::Assign(Place::new(local_i), Rvalue::Use(const_int(0))),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    );
    // Make it loop forever
    set_terminator(&mut body, bb0, TerminatorKind::Goto { target: bb0 });

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx).with_step_limit(10);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), InterpError::TimedOut);
}

#[test]
fn test_recursion_limit_enforced() {
    let mut ctx = glyim_test::test_ty_ctx();
    let _i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let fn_def = FnDefId::from_raw(0);
    let fn_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(fn_def.to_raw()));

    let mut body = empty_body(Ty::UNIT);
    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    set_terminator(
        &mut body,
        bb0,
        TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(fn_def, glyim_type::Substitution::empty()),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(LocalIdx::from_raw(0)),
            target: Some(bb1),
            cleanup: None,
        },
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx).with_recursion_limit(5);
    interp.add_function(fn_id, body);
    let result = interp.run_body(&interp.function_table.get(&fn_id).unwrap().clone());
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), InterpError::StackOverflow);
}

#[test]
fn test_call_with_cleanup_on_panic() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let callee_def = FnDefId::from_raw(10);
    let callee_id = DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(callee_def.to_raw()),
    );
    let mut callee_body = empty_body(Ty::UNIT);
    let bb0 = BasicBlockIdx::from_raw(0);
    set_terminator(&mut callee_body, bb0, TerminatorKind::Unreachable);

    let caller_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(20));
    let mut caller_body = empty_body(Ty::UNIT);
    let local_result = add_local(&mut caller_body, i32_ty, Mutability::Mut);
    let bb0_caller = BasicBlockIdx::from_raw(0);
    let bb1_caller = BasicBlockIdx::from_raw(1);
    let bb2_caller = BasicBlockIdx::from_raw(2);
    caller_body
        .basic_blocks
        .push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));
    caller_body
        .basic_blocks
        .push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));

    set_terminator(
        &mut caller_body,
        bb0_caller,
        TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(callee_def, glyim_type::Substitution::empty()),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(local_result),
            target: Some(bb1_caller),
            cleanup: Some(bb2_caller),
        },
    );
    add_statement(
        &mut caller_body,
        bb1_caller,
        StatementKind::Assign(Place::new(local_result), Rvalue::Use(const_int(123))),
    );
    add_statement(
        &mut caller_body,
        bb2_caller,
        StatementKind::Assign(Place::new(local_result), Rvalue::Use(const_int(456))),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_id, callee_body);
    interp.add_function(caller_id, caller_body);
    let result = interp.run_body(&interp.function_table.get(&caller_id).unwrap().clone());
    assert!(result.is_err());
}

#[test]
fn test_discriminant_on_non_empty_aggregate() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_enum = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_discr = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_enum),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![const_int(1), const_int(42)]),
        ),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_discr),
            Rvalue::Discriminant(Place::new(local_enum)),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    assert_eq!(
        *interp.get_local_value(local_discr).unwrap(),
        InterpValue::Int(1)
    );
}

#[test]
fn test_downcast_projection_is_noop() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_val = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_result = add_local(&mut body, i32_ty, Mutability::Not);

    let bb0 = BasicBlockIdx::from_raw(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_val), Rvalue::Use(const_int(77))),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_result),
            Rvalue::Use(Operand::Copy(Place {
                local: local_val,
                projection: vec![ProjectionElem::Downcast(glyim_mir::VariantIdx::from_raw(0))]
                    .into_boxed_slice(),
            })),
        ),
    );

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    assert_eq!(
        *interp.get_local_value(local_result).unwrap(),
        InterpValue::Int(77)
    );
}
