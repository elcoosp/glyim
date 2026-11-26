use super::helpers::*;
use crate::{InterpValue, Interpreter};
use glyim_core::*;
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Ty, TyKind};

#[test]
fn test_t10_nested_function_calls() {
    let mut ctx = glyim_test::test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    // Callee body: function add(x: i32, y: i32) -> i32 { x + y }
    let callee_fn_def = FnDefId::from_usize(10);
    let callee_def_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(callee_fn_def.index()),
    );
    let mut callee_body = empty_body(i32_ty);
    // local 0 is return place (already), local 1 and 2 for params
    let param0 = add_local(&mut callee_body, i32_ty, Mutability::Not);
    let param1 = add_local(&mut callee_body, i32_ty, Mutability::Not);
    callee_body.arg_count = 2;
    // Need to set return place; we'll assign result to local 0
    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut callee_body,
        bb0,
        StatementKind::Assign(
            Place::new(LocalIdx::from_usize(0)),
            Rvalue::BinaryOp(
                BinOp::Add,
                [
                    Operand::Copy(Place::new(param0)),
                    Operand::Copy(Place::new(param1)),
                ],
            ),
        ),
    );

    // Caller body: call add(3,4) and store in local
    let caller_def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(20));
    let mut caller_body = empty_body(Ty::UNIT);
    let local_result = add_local(&mut caller_body, i32_ty, Mutability::Mut);
    let dest = Place::new(local_result);
    let bb0_caller = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut caller_body,
        bb0_caller,
        StatementKind::Assign(
            Place::new(local_result),
            Rvalue::Use(const_int(0)), // placeholder, will be overwritten by Call's destination
        ),
    );
    // Build the Call terminator
    set_terminator(
        &mut caller_body,
        bb0_caller,
        TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Fn(callee_fn_def, glyim_type::Substitution::empty()),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            }),
            args: vec![const_int(3), const_int(4)],
            destination: dest,
            target: Some(BasicBlockIdx::from_usize(1)),
            cleanup: None,
        },
    );
    // Add a block for after call to just return
    let _bb1_caller = BasicBlockIdx::from_usize(1);
    caller_body
        .basic_blocks
        .push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        }));

    let tcx = ctx.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.add_function(callee_def_id, callee_body);
    interp.add_function(caller_def_id, caller_body);

    let result = interp.run_body(&interp.function_table.get(&caller_def_id).unwrap().clone());
    assert!(result.is_ok());
    let val = interp.get_local_value(local_result).unwrap();
    assert_eq!(*val, InterpValue::Int(7));
}
