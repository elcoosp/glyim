use super::helpers::*;
use crate::{InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyKind};

#[test]
fn test_t08_while_loop() {
    // Simulate: let mut i = 0; while i < 3 { i += 1; }
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_i = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_cond = add_local(&mut body, Ty::BOOL, Mutability::Mut);

    // Blocks: bb0 = initialization, bb1 = header (check condition), bb2 = body, bb3 = exit
    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let bb2 = BasicBlockIdx::from_raw(2);
    let bb3 = BasicBlockIdx::from_raw(3);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb2 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb3 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    // bb0: i = 0; goto bb1
    body.basic_blocks[bb0].statements.clear();
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_i), Rvalue::Use(const_int(0))),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Goto { target: bb1 });

    // bb1: cond = i < 3; switch cond -> bb2 if true, bb3 if false
    body.basic_blocks[bb1].statements.clear();
    add_statement(
        &mut body,
        bb1,
        StatementKind::Assign(
            Place::new(local_cond),
            Rvalue::BinaryOp(
                BinOp::Lt,
                [Operand::Copy(Place::new(local_i)), const_int(3)],
            ),
        ),
    );
    set_terminator(
        &mut body,
        bb1,
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(local_cond)),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::if_switch(bb2, bb3),
        },
    );

    // bb2: i = i + 1; goto bb1
    body.basic_blocks[bb2].statements.clear();
    add_statement(
        &mut body,
        bb2,
        StatementKind::Assign(
            Place::new(local_i),
            Rvalue::BinaryOp(
                BinOp::Add,
                [Operand::Copy(Place::new(local_i)), const_int(1)],
            ),
        ),
    );
    set_terminator(&mut body, bb2, TerminatorKind::Goto { target: bb1 });

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    let final_i = interp.get_local_value(local_i).unwrap();
    assert_eq!(*final_i, InterpValue::Int(3));
}

#[test]
fn test_t09_break_and_continue() {
    // Simulate a loop with break: while i < 10 { if i == 5 { break; } i += 1; }
    // We'll simplify: i starts 0; loop: if i>=5 goto exit; i+=1; goto loop;
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut body = empty_body(Ty::UNIT);
    let local_i = add_local(&mut body, i32_ty, Mutability::Mut);
    let local_cond = add_local(&mut body, Ty::BOOL, Mutability::Mut);

    // Blocks: bb0 = initialization, bb1 = header, bb2 = body increment, bb3 = exit
    let bb0 = BasicBlockIdx::from_raw(0);
    let bb1 = BasicBlockIdx::from_raw(1);
    let bb2 = BasicBlockIdx::from_raw(2);
    let bb3 = BasicBlockIdx::from_raw(3);
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb1 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto { target: bb3 },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    // bb0: i=0; goto bb1
    body.basic_blocks[bb0].statements.clear();
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_i), Rvalue::Use(const_int(0))),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Goto { target: bb1 });

    // bb1: cond = i>=5; switch cond true->exit, false->body
    body.basic_blocks[bb1].statements.clear();
    add_statement(
        &mut body,
        bb1,
        StatementKind::Assign(
            Place::new(local_cond),
            Rvalue::BinaryOp(
                BinOp::GtEq,
                [Operand::Copy(Place::new(local_i)), const_int(5)],
            ),
        ),
    );
    set_terminator(
        &mut body,
        bb1,
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(local_cond)),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::if_switch(bb3, bb2),
        },
    );

    // bb2: i=i+1; goto bb1
    body.basic_blocks[bb2].statements.clear();
    add_statement(
        &mut body,
        bb2,
        StatementKind::Assign(
            Place::new(local_i),
            Rvalue::BinaryOp(
                BinOp::Add,
                [Operand::Copy(Place::new(local_i)), const_int(1)],
            ),
        ),
    );
    set_terminator(&mut body, bb2, TerminatorKind::Goto { target: bb1 });

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(
        DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        body,
    );
    let result = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(result.is_ok());
    let final_i = interp.get_local_value(local_i).unwrap();
    assert_eq!(*final_i, InterpValue::Int(5));
}
