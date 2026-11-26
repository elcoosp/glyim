use crate::InterpValue;
use crate::Interpreter;
use glyim_core::{CrateId, DefId, FloatTy, IndexVec, IntTy, LocalDefId, Mutability, UintTy};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, GenericArg, Ty, TyCtxMut, TyKind};

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0))
}

fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

fn empty_body(ret_ty: Ty) -> Body {
    let mut body = Body::dummy(dummy_def_id());
    body.locals = IndexVec::from_vec(vec![local_decl(ret_ty, Mutability::Mut)]);
    body
}

fn add_local(body: &mut Body, ty: Ty, mutability: Mutability) -> LocalIdx {
    let idx = LocalIdx::from_usize(body.locals.len());
    body.locals.push(local_decl(ty, mutability));
    idx
}

fn add_statement(body: &mut Body, bb: BasicBlockIdx, stmt: StatementKind) {
    while body.basic_blocks.len() <= bb.index() {
        body.basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        });
    }
    body.basic_blocks[bb].statements.push(Statement {
        kind: stmt,
        source_info: SourceInfo::new(Span::DUMMY),
    });
}

fn set_terminator(body: &mut Body, bb: BasicBlockIdx, kind: TerminatorKind) {
    while body.basic_blocks.len() <= bb.index() {
        body.basic_blocks.push(BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        });
    }
    body.basic_blocks[bb].terminator.kind = kind;
}

fn const_int(_tcx: &mut TyCtxMut, val: i128, ty: Ty) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty,
        span: Span::DUMMY,
    })
}

fn mk_array_ty(tcx: &mut TyCtxMut, elem_ty: Ty, len: u64) -> Ty {
    let usize_ty = tcx.mk_ty(TyKind::Uint(UintTy::Usize));
    let const_len = Const {
        kind: ConstKind::Uint(len.into()),
        ty: usize_ty,
    };
    tcx.mk_ty(TyKind::Array(elem_ty, const_len))
}

#[test]
fn discriminant_returns_tag() {
    let mut tcx = glyim_test::test_ty_ctx();
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let tuple_substs =
        tcx.intern_substitution(vec![GenericArg::Ty(int_ty), GenericArg::Ty(int_ty)]);
    let tuple_ty = tcx.mk_ty(TyKind::Tuple(tuple_substs));
    let mut body = empty_body(Ty::UNIT);
    let local_enum = add_local(&mut body, tuple_ty, Mutability::Mut);
    let local_result = add_local(&mut body, int_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    let agg = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![
            const_int(&mut tcx, 42, int_ty),
            const_int(&mut tcx, 0, int_ty),
        ],
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_enum), agg),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local_result),
            Rvalue::Discriminant(Place::new(local_enum)),
        ),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local_result).unwrap();
    assert_eq!(val, &InterpValue::Int(42));
}

#[test]
fn cast_int_to_float() {
    let mut tcx = glyim_test::test_ty_ctx();
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let float_ty = tcx.mk_ty(TyKind::Float(FloatTy::F64));
    let mut body = empty_body(Ty::UNIT);
    let local = add_local(&mut body, float_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local),
            Rvalue::Cast(
                CastKind::IntToFloat,
                const_int(&mut tcx, 42, int_ty),
                float_ty,
            ),
        ),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local).unwrap();
    assert_eq!(val, &InterpValue::Float(42.0));
}

#[test]
fn cast_float_to_int() {
    let mut tcx = glyim_test::test_ty_ctx();
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let float_ty = tcx.mk_ty(TyKind::Float(FloatTy::F64));
    let mut body = empty_body(Ty::UNIT);
    let local = add_local(&mut body, int_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    let float_const = MirConst {
        kind: MirConstKind::FloatBits(123.456_f64.to_bits()),
        ty: float_ty,
        span: Span::DUMMY,
    };
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(
            Place::new(local),
            Rvalue::Cast(CastKind::FloatToInt, Operand::Constant(float_const), int_ty),
        ),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local).unwrap();
    assert_eq!(val, &InterpValue::Int(123));
}

#[test]
fn repeat_creates_array() {
    let mut tcx = glyim_test::test_ty_ctx();
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = mk_array_ty(&mut tcx, int_ty, 5);
    let mut body = empty_body(Ty::UNIT);
    let local = add_local(&mut body, array_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    let repeat = Rvalue::Repeat(
        const_int(&mut tcx, 42, int_ty),
        MirConst {
            kind: MirConstKind::Uint(5),
            ty: tcx.mk_ty(TyKind::Uint(UintTy::Usize)),
            span: Span::DUMMY,
        },
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local), repeat),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local).unwrap();
    let expected = InterpValue::Aggregate(vec![InterpValue::Int(42); 5]);
    assert_eq!(val, &expected);
}

#[test]
fn len_of_array() {
    let mut tcx = glyim_test::test_ty_ctx();
    let int_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = mk_array_ty(&mut tcx, int_ty, 7);
    let mut body = empty_body(Ty::UNIT);
    let local_array = add_local(&mut body, array_ty, Mutability::Mut);
    let local_len = add_local(
        &mut body,
        tcx.mk_ty(TyKind::Uint(UintTy::Usize)),
        Mutability::Mut,
    );
    let bb0 = BasicBlockIdx::from_usize(0);
    let init = Rvalue::Repeat(
        const_int(&mut tcx, 0, int_ty),
        MirConst {
            kind: MirConstKind::Uint(7),
            ty: tcx.mk_ty(TyKind::Uint(UintTy::Usize)),
            span: Span::DUMMY,
        },
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_array), init),
    );
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local_len), Rvalue::Len(Place::new(local_array))),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local_len).unwrap();
    assert_eq!(val, &InterpValue::Int(7));
}

#[test]
fn fn_constant_used_as_operand() {
    let tcx = glyim_test::test_ty_ctx();
    let fn_def_id = glyim_core::def_id::FnDefId::from_usize(100);
    let dummy_ty = Ty::UNIT;
    let mut body = empty_body(Ty::UNIT);
    let local = add_local(&mut body, dummy_ty, Mutability::Mut);
    let bb0 = BasicBlockIdx::from_usize(0);
    let const_val = MirConst {
        kind: MirConstKind::Fn(fn_def_id, glyim_type::Substitution::empty()),
        ty: dummy_ty,
        span: Span::DUMMY,
    };
    add_statement(
        &mut body,
        bb0,
        StatementKind::Assign(Place::new(local), Rvalue::Use(Operand::Constant(const_val))),
    );
    set_terminator(&mut body, bb0, TerminatorKind::Return);
    let tcx_frozen = tcx.freeze();
    let mut interp = Interpreter::new(&tcx_frozen);
    interp.add_function(dummy_def_id(), body);
    let res = interp.run_body(&interp.function_table.values().next().unwrap().clone());
    assert!(res.is_ok());
    let val = interp.get_local_value(local).unwrap();
    let expected_def_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(fn_def_id.index()),
    );
    assert_eq!(val, &InterpValue::Fn(expected_def_id));
}
