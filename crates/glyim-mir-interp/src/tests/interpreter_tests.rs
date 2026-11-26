use crate::{InterpError, InterpResult, InterpValue, Interpreter};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, IntTy, LocalDefId, Mutability, UnOp};
use glyim_mir::*;
use glyim_span::Span;
use glyim_test::{test_ty_ctx, with_fresh_ty_ctx};
use glyim_type::{ConstKind, Ty, TyKind};

fn create_const_int(value: i128, ty: Ty) -> MirConst {
    MirConst {
        kind: MirConstKind::Int(value),
        ty,
        span: Span::DUMMY,
    }
}

fn create_const_bool(value: bool) -> MirConst {
    MirConst {
        kind: MirConstKind::Bool(value),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    }
}

fn eval_single_rvalue(
    ctx: &glyim_type::TyCtx,
    rvalue: Rvalue,
    return_type: Ty,
) -> InterpResult<InterpValue> {
    let mut interp = Interpreter::new(ctx);
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_type,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let statements = vec![Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(0)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    }];
    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let basic_blocks = IndexVec::from_vec(vec![BasicBlockData {
        statements,
        terminator,
        is_cleanup: false,
    }]);
    let body = Body {
        owner: def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: return_type,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };
    interp.add_function(def_id, body.clone());
    interp.run_body(&body)?;
    interp
        .get_return_value()
        .ok_or(InterpError::Panic("no return value".into()))
}

#[test]
fn test_shift_left() {
    let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let left = Operand::Constant(create_const_int(1, int_ty));
    let right = Operand::Constant(create_const_int(2, int_ty));
    let rvalue = Rvalue::binary_op(BinOp::Shl, left, right);
    let result = eval_single_rvalue(&ctx, rvalue, int_ty).unwrap();
    assert_eq!(result, InterpValue::Int(4));
}

#[test]
fn test_not_bool() {
    let ctx = test_ty_ctx().freeze();
    let operand = Operand::Constant(create_const_bool(true));
    let rvalue = Rvalue::UnaryOp(UnOp::Not, operand);
    let result = eval_single_rvalue(&ctx, rvalue, Ty::BOOL).unwrap();
    assert_eq!(result, InterpValue::Bool(false));
}

#[test]
fn test_not_int() {
    let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let operand = Operand::Constant(create_const_int(0b1010, int_ty));
    let rvalue = Rvalue::UnaryOp(UnOp::Not, operand);
    let result = eval_single_rvalue(&ctx, rvalue, int_ty).unwrap();
    assert_eq!(result, InterpValue::Int(!10i128));
}

#[test]
fn test_len_array() {
    let (ctx, (int_ty, array_ty)) = with_fresh_ty_ctx(|c| {
        let int_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let array_ty = c.mk_ty(TyKind::Array(
            int_ty,
            glyim_type::Const {
                kind: ConstKind::Int(3),
                ty: int_ty,
            },
        ));
        (int_ty, array_ty)
    });
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let init_array = Rvalue::Aggregate(
        AggregateKind::Array(int_ty),
        vec![
            Operand::Constant(create_const_int(0, int_ty)),
            Operand::Constant(create_const_int(0, int_ty)),
            Operand::Constant(create_const_int(0, int_ty)),
        ],
    );
    let statements = vec![
        Statement {
            kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(1)), init_array),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Len(Place::new(LocalIdx::from_usize(1))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let basic_blocks = IndexVec::from_vec(vec![BasicBlockData {
        statements,
        terminator,
        is_cleanup: false,
    }]);
    let body = Body {
        owner: def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: int_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };
    let mut interp = Interpreter::new(&ctx);
    interp.add_function(def_id, body.clone());
    interp.run_body(&body).unwrap();
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(3));
}

#[test]
fn test_aggregate_and_field_projection() {
    // Create a proper tuple type with two i32 fields using mutable context
    let (ctx, (int_ty, tuple_ty)) = with_fresh_ty_ctx(|c| {
        let int_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        // Create substitution with two i32 types for the tuple
        let subst = c.intern_substitution(vec![
            glyim_type::GenericArg::Ty(int_ty),
            glyim_type::GenericArg::Ty(int_ty),
        ]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(subst));
        (int_ty, tuple_ty)
    });
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let init_tuple = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![
            Operand::Constant(create_const_int(1, int_ty)),
            Operand::Constant(create_const_int(2, int_ty)),
        ],
    );
    let tuple_place = Place::new(LocalIdx::from_usize(2));
    let field_place = Place {
        local: LocalIdx::from_usize(2),
        projection: vec![ProjectionElem::Field(glyim_type::FieldIdx::from_usize(0))]
            .into_boxed_slice(),
    };
    let statements = vec![
        Statement {
            kind: StatementKind::Assign(tuple_place, init_tuple),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                field_place.clone(),
                Rvalue::Use(Operand::Constant(create_const_int(42, int_ty))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Use(Operand::Copy(field_place)),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let basic_blocks = IndexVec::from_vec(vec![BasicBlockData {
        statements,
        terminator,
        is_cleanup: false,
    }]);
    let body = Body {
        owner: def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: int_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };
    let mut interp = Interpreter::new(&ctx);
    interp.add_function(def_id, body.clone());
    interp.run_body(&body).unwrap();
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(42));
}

#[test]
fn test_switch_int_multiple_targets() {
    let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let discr_operand = Operand::Constant(create_const_int(2, int_ty));
    let targets = SwitchTargets::new(
        Box::new([
            (0, BasicBlockIdx::from_usize(1)),
            (1, BasicBlockIdx::from_usize(2)),
            (2, BasicBlockIdx::from_usize(3)),
        ]),
        BasicBlockIdx::from_usize(4),
    );
    let terminator = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: discr_operand,
            switch_ty: int_ty,
            targets,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block0 = BasicBlockData {
        statements: vec![],
        terminator,
        is_cleanup: false,
    };
    let block1 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Use(Operand::Constant(create_const_int(0, int_ty))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let block2 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Use(Operand::Constant(create_const_int(1, int_ty))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let block3 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Use(Operand::Constant(create_const_int(2, int_ty))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let block4 = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Use(Operand::Constant(create_const_int(3, int_ty))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(block0);
    basic_blocks.push(block1);
    basic_blocks.push(block2);
    basic_blocks.push(block3);
    basic_blocks.push(block4);
    let body = Body {
        owner: def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: int_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };
    let mut interp = Interpreter::new(&ctx);
    interp.add_function(def_id, body.clone());
    interp.run_body(&body).unwrap();
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(2));
}

#[test]
fn test_discriminant_enum() {
    let (ctx, int_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    let disc = Operand::Constant(create_const_int(0, int_ty));
    let data = Operand::Constant(create_const_int(5, int_ty));
    let agg = Rvalue::Aggregate(AggregateKind::Tuple, vec![disc, data]);
    let def_id = DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0));
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: int_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let statements = vec![
        Statement {
            kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(1)), agg),
            source_info: SourceInfo::new(Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_usize(0)),
                Rvalue::Discriminant(Place::new(LocalIdx::from_usize(1))),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        },
    ];
    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let basic_blocks = IndexVec::from_vec(vec![BasicBlockData {
        statements,
        terminator,
        is_cleanup: false,
    }]);
    let body = Body {
        owner: def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: int_ty,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    };
    let mut interp = Interpreter::new(&ctx);
    interp.add_function(def_id, body.clone());
    interp.run_body(&body).unwrap();
    let ret = interp.get_return_value().unwrap();
    assert_eq!(ret, InterpValue::Int(0));
}
