use crate::{InterpValue, Interpreter};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, LocalDefId, Mutability};
use glyim_mir::*;
use glyim_type::Ty;

/// Helper: build a body that computes `left_val` OP `right_val` and returns the result.
fn build_binop_body_int(op: BinOp, left: i128, right: i128) -> Body {
    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(3);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let return_place = Place::new(LocalIdx::from_raw(0));
    let left_place = Place::new(LocalIdx::from_raw(1));
    let right_place = Place::new(LocalIdx::from_raw(2));

    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                left_place,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(left),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                right_place,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(right),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                return_place,
                Rvalue::BinaryOp(
                    op,
                    Box::new((
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                    )),
                ),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: stmts,
        terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: build a body for uint binop
fn build_binop_body_uint(op: BinOp, left: u128, right: u128) -> Body {
    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(3);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let return_place = Place::new(LocalIdx::from_raw(0));

    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(left),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(right),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                return_place,
                Rvalue::BinaryOp(
                    op,
                    Box::new((
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                    )),
                ),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: stmts,
        terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

/// Helper: build a body for bool binop
fn build_binop_body_bool(op: BinOp, left: bool, right: bool) -> Body {
    let crate_id = CrateId::from_raw(0);
    let local_def_id = LocalDefId::from_raw(0);
    let owner = DefId::new(crate_id, local_def_id);

    let mut locals = IndexVec::with_capacity(3);
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    });

    let return_place = Place::new(LocalIdx::from_raw(0));

    let stmts = vec![
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(1)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(left),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(right),
                    ty: Ty::UNIT,
                    span: glyim_span::Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
        Statement {
            kind: StatementKind::Assign(
                return_place,
                Rvalue::BinaryOp(
                    op,
                    Box::new((
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                    )),
                ),
            ),
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        },
    ];

    let terminator = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(glyim_span::Span::DUMMY),
    };

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(BasicBlockData {
        statements: stmts,
        terminator,
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: glyim_span::Span::DUMMY,
        var_debug_info: vec![],
    }
}

fn run_int_body(body: &Body) -> InterpValue {
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(body);
    assert!(result.is_ok(), "run_body failed: {result:?}");
    interp
        .get_local_value(LocalIdx::from_raw(0))
        .cloned()
        .unwrap()
}

#[test]
fn int_bitand() {
    let body = build_binop_body_int(BinOp::BitAnd, 0b1100, 0b1010);
    assert_eq!(run_int_body(&body), InterpValue::Int(0b1000));
}

#[test]
fn int_bitor() {
    let body = build_binop_body_int(BinOp::BitOr, 0b1100, 0b1010);
    assert_eq!(run_int_body(&body), InterpValue::Int(0b1110));
}

#[test]
fn int_bitxor() {
    let body = build_binop_body_int(BinOp::BitXor, 0b1100, 0b1010);
    assert_eq!(run_int_body(&body), InterpValue::Int(0b0110));
}

#[test]
fn int_shl() {
    let body = build_binop_body_int(BinOp::Shl, 1, 4);
    assert_eq!(run_int_body(&body), InterpValue::Int(16));
}

#[test]
fn int_shr() {
    let body = build_binop_body_int(BinOp::Shr, 16, 4);
    assert_eq!(run_int_body(&body), InterpValue::Int(1));
}

#[test]
fn uint_bitand() {
    let body = build_binop_body_uint(BinOp::BitAnd, 0b1100, 0b1010);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Uint(0b1000)
    );
}

#[test]
fn uint_bitor() {
    let body = build_binop_body_uint(BinOp::BitOr, 0b1100, 0b1010);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Uint(0b1110)
    );
}

#[test]
fn uint_bitxor() {
    let body = build_binop_body_uint(BinOp::BitXor, 0b1100, 0b1010);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Uint(0b0110)
    );
}

#[test]
fn uint_shl() {
    let body = build_binop_body_uint(BinOp::Shl, 1, 4);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Uint(16)
    );
}

#[test]
fn uint_shr() {
    let body = build_binop_body_uint(BinOp::Shr, 16, 4);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Uint(1)
    );
}

#[test]
fn bool_and() {
    let body = build_binop_body_bool(BinOp::And, true, false);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Bool(false)
    );
}

#[test]
fn bool_or() {
    let body = build_binop_body_bool(BinOp::Or, true, false);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Bool(true)
    );
}

#[test]
fn bool_and_both_true() {
    let body = build_binop_body_bool(BinOp::And, true, true);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Bool(true)
    );
}

#[test]
fn bool_or_both_false() {
    let body = build_binop_body_bool(BinOp::Or, false, false);
    let ctx = glyim_test::test_frozen_ty_ctx();
    let mut interp = Interpreter::new(&ctx);
    let result = interp.run_body(&body);
    assert!(result.is_ok());
    assert_eq!(
        interp
            .get_local_value(LocalIdx::from_raw(0))
            .cloned()
            .unwrap(),
        InterpValue::Bool(false)
    );
}

/// S19-T03b: Comparison operators on integers
#[test]
fn int_eq() {
    let body = build_binop_body_int(BinOp::Eq, 42, 42);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}

#[test]
fn int_ne() {
    let body = build_binop_body_int(BinOp::Ne, 42, 43);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}

#[test]
fn int_lt() {
    let body = build_binop_body_int(BinOp::Lt, 10, 20);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}

#[test]
fn int_gt() {
    let body = build_binop_body_int(BinOp::Gt, 20, 10);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}

#[test]
fn int_lteq() {
    let body = build_binop_body_int(BinOp::LtEq, 10, 10);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}

#[test]
fn int_gteq() {
    let body = build_binop_body_int(BinOp::GtEq, 10, 10);
    assert_eq!(run_int_body(&body), InterpValue::Bool(true));
}
