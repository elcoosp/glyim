use crate::{InterpValue, Interpreter};
use glyim_core::{CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_type::{Ty, TyKind};

/// S19-T02: Len evaluates array length from const generic
#[test]
fn len_evaluates_array_length() {
    let (ctx, array_ty) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let count = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(4),
            ty: i32_ty,
        };
        ctx_mut.mk_ty(TyKind::Array(i32_ty, count))
    });

    let mut interp = Interpreter::new(&ctx);

    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(2);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let array_place = Place::new(LocalIdx::from_raw(1));
    let return_place = Place::new(LocalIdx::from_raw(0));

    let agg_stmt = Statement {
        kind: StatementKind::Assign(
            array_place.clone(),
            Rvalue::Aggregate(
                AggregateKind::Array(Ty::UNIT),
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(20),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(30),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(40),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let len_stmt = Statement {
        kind: StatementKind::Assign(return_place.clone(), Rvalue::Len(array_place)),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![agg_stmt, len_stmt],
        terminator,
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = interp.run_body(&body);
    assert!(result.is_ok(), "run_body failed: {result:?}");

    let ret = interp.get_local_value(LocalIdx::from_raw(0));
    assert!(ret.is_some(), "return place not set");
    assert_eq!(
        ret.unwrap(),
        &InterpValue::Int(4),
        "Len should return array length 4"
    );
}

/// S19-T02b: Len on slice-typed aggregate returns element count
#[test]
fn len_on_aggregate_slice_returns_count() {
    let (ctx, array_ty) = glyim_test::with_fresh_ty_ctx(|ctx_mut| {
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let count = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(3),
            ty: i32_ty,
        };
        ctx_mut.mk_ty(TyKind::Array(i32_ty, count))
    });
    let mut interp = Interpreter::new(&ctx);

    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(2);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let array_place = Place::new(LocalIdx::from_raw(1));
    let return_place = Place::new(LocalIdx::from_raw(0));

    let agg_stmt = Statement {
        kind: StatementKind::Assign(
            array_place.clone(),
            Rvalue::Aggregate(
                AggregateKind::Array(Ty::UNIT),
                vec![
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(2),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(3),
                        ty: Ty::UNIT,
                        span: glyim_span::Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let len_stmt = Statement {
        kind: StatementKind::Assign(return_place.clone(), Rvalue::Len(array_place)),
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: vec![agg_stmt, len_stmt],
        terminator,
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    };

    let result = interp.run_body(&body);
    assert!(result.is_ok(), "run_body failed: {result:?}");

    let ret = interp.get_local_value(LocalIdx::from_raw(0));
    assert!(ret.is_some(), "return place not set");
    assert_eq!(ret.unwrap(), &InterpValue::Int(3), "Len should return 3");
}
