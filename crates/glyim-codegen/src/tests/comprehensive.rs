use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, Mutability, UnOp};
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

fn dummy_body() -> Body {
    Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)))
}

// ─── Binary Ops Edge Cases ───

#[test]
fn binary_op_zero_values() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new((
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(0),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(0),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                    )),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_ADD));
}

#[test]
fn binary_op_negative_values() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::BinaryOp(
                    BinOp::Mul,
                    Box::new((
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(-1),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(-42),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                    )),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    assert!(result.unwrap().contains(&OP_MUL));
}

#[test]
fn binary_op_large_values() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::BinaryOp(
                    BinOp::BitAnd,
                    Box::new((
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(i128::from(i64::MAX)),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(i128::from(i64::MIN)),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                    )),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    assert!(result.unwrap().contains(&OP_BITAND));
}

// ─── Uint and Float Constants ───

#[test]
fn uint_constant_emission() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(42),
                    ty: Ty::ERROR,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    // Should start with OP_LOAD_CONST
    assert_eq!(bc[0], OP_LOAD_CONST);
}

#[test]
fn float_constant_emission() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let pi_bits = std::f64::consts::PI.to_bits();
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::FloatBits(pi_bits),
                    ty: Ty::ERROR,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
}

#[test]
fn char_constant_emission() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Char('A'),
                    ty: Ty::ERROR,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
}

// ─── Aggregate Variants ───

#[test]
fn aggregate_array() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let operands = vec![
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(10),
            ty: Ty::ERROR,
            span: Span::DUMMY,
        }),
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(20),
            ty: Ty::ERROR,
            span: Span::DUMMY,
        }),
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(30),
            ty: Ty::ERROR,
            span: Span::DUMMY,
        }),
    ];
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Aggregate(AggregateKind::Array(Ty::ERROR), operands),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_AGGREGATE));
    let pos = bc.iter().position(|&b| b == OP_AGGREGATE).unwrap();
    let count = u32::from_le_bytes([bc[pos + 1], bc[pos + 2], bc[pos + 3], bc[pos + 4]]);
    assert_eq!(count, 3);
}

#[test]
fn aggregate_adt() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let operands = vec![Operand::Constant(MirConst {
        kind: MirConstKind::Int(1),
        ty: Ty::ERROR,
        span: Span::DUMMY,
    })];
    // Create an empty Substitution via TyCtxMut
    let mut tcx = glyim_test::test_ty_ctx();
    let empty_substs = tcx.intern_substitution(vec![]);
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Aggregate(
                    AggregateKind::Adt(
                        glyim_core::def_id::AdtId::from_raw(0),
                        glyim_mir::VariantIdx::from_raw(0),
                        empty_substs,
                    ),
                    operands,
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    assert!(result.unwrap().contains(&OP_AGGREGATE));
}

// ─── Cast Kind Bytes ───

#[test]
fn cast_kind_bytes() {
    let cast_kinds = vec![
        (CastKind::IntToInt, 0u8),
        (CastKind::FloatToInt, 1u8),
        (CastKind::IntToFloat, 2u8),
        (CastKind::PtrToPtr, 3u8),
        (CastKind::FnPtrToPtr, 4u8),
    ];
    for (kind, expected_byte) in cast_kinds {
        let mut body = dummy_body();
        let dest = LocalIdx::from_raw(1);
        body.locals.push(LocalDecl {
            ty: Ty::ERROR,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
        body.basic_blocks[BasicBlockIdx::from_raw(0)]
            .statements
            .push(Statement {
                kind: StatementKind::Assign(
                    Place::new(dest),
                    Rvalue::Cast(
                        kind,
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Int(1),
                            ty: Ty::ERROR,
                            span: Span::DUMMY,
                        }),
                        Ty::ERROR,
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            });
        let backend = BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(
                glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze(),
            ),
            glyim_core::TargetInfo::default(),
        );
        let result = backend.generate_function(&Arc::new(body));
        assert!(result.is_ok(), "Cast kind {kind:?} should succeed");
        let bc = result.unwrap();
        let cast_pos = bc
            .iter()
            .position(|&b| b == OP_CAST)
            .expect("OP_CAST not found");
        assert_eq!(
            bc[cast_pos + 1],
            expected_byte,
            "Cast kind {kind:?} should emit byte {expected_byte}"
        );
    }
}

// ─── Repeat Rvalue ───

#[test]
fn emit_repeat() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Repeat(
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(7),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    MirConst {
                        kind: MirConstKind::Uint(3),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    },
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Repeat should succeed");
}

// ─── Multiple Statements ───

#[test]
fn multiple_statements_in_block() {
    let mut body = dummy_body();
    let a = LocalIdx::from_raw(1);
    let b = LocalIdx::from_raw(2);
    let c = LocalIdx::from_raw(3);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let block = &mut body.basic_blocks[BasicBlockIdx::from_raw(0)];
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(a),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(1),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(b),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(2),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(c),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new((Operand::Copy(Place::new(a)), Operand::Copy(Place::new(b)))),
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    // Parse bytecode properly (byte count includes operand bytes, avoid false positives)
    let mut load_const_cnt = 0u32;
    let mut load_local_cnt = 0u32;
    let mut has_add = false;
    let mut pos = 0usize;
    while pos < bc.len() {
        match bc[pos] {
            OP_LOAD_CONST => {
                load_const_cnt += 1;
                pos += 1 + 8; // opcode + i64
            }
            OP_LOAD_LOCAL => {
                load_local_cnt += 1;
                pos += 1 + 4; // opcode + u32 local
            }
            OP_STORE_LOCAL => {
                pos += 1 + 4;
            }
            OP_ADD => {
                has_add = true;
                pos += 1;
            }
            OP_JUMP | OP_JUMP_IF | OP_RETURN | OP_CALL | OP_CAST | OP_AGGREGATE
            | OP_DISCRIMINANT | OP_LEN | OP_SWITCH_INT | OP_ASSERT => {
                // skip unhandled opcodes with their arguments for simplicity; just stop parsing
                break;
            }
            _ => {
                // unknown opcode, stop parsing
                break;
            }
        }
    }
    assert_eq!(load_const_cnt, 2, "Two literal constants");
    assert_eq!(load_local_cnt, 2, "Two local loads (a, b)");
    assert!(has_add, "Contains OP_ADD");
}

// ─── Multiple Basic Blocks ───

#[test]
fn multiple_basic_blocks_with_goto() {
    let mut body = dummy_body();
    let a = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    // BB0: assign 1 to a, goto BB1
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(a),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: Ty::ERROR,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(1),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    // BB1: return
    body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_JUMP), "Should have OP_JUMP for Goto");
    assert!(bc.contains(&OP_RETURN), "Should have OP_RETURN for Return");
}

// ─── Call with Cleanup ───

#[test]
fn call_with_cleanup() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Call {
            func: Operand::Constant(MirConst {
                kind: MirConstKind::Int(0),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            }),
            args: vec![],
            destination: Place::new(dest),
            target: Some(BasicBlockIdx::from_raw(1)),
            cleanup: Some(BasicBlockIdx::from_raw(2)),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Call with cleanup should succeed");
    assert!(result.unwrap().contains(&OP_CALL));
}

// ─── Switch on Bool (existing code path) ───

#[test]
fn switch_int_bool() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Constant(MirConst {
                kind: MirConstKind::Bool(true),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::new(
                Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                BasicBlockIdx::from_raw(2),
            ),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Bool switch should succeed");
    let bc = result.unwrap();
    assert!(
        bc.contains(&OP_JUMP_IF),
        "Should have OP_JUMP_IF for bool switch"
    );
}

// ─── Unary Deref Error ───

#[test]
fn unary_deref_succeeds() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::UnaryOp(
                    UnOp::Deref,
                    Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok(), "Deref should succeed (implemented)");
}

// ─── Assert with False Expected ───

#[test]
fn assert_false_expected() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Assert {
            cond: Operand::Constant(MirConst {
                kind: MirConstKind::Bool(false),
                ty: Ty::BOOL,
                span: Span::DUMMY,
            }),
            expected: false,
            target: BasicBlockIdx::from_raw(1),
            cleanup: None,
            msg: AssertMessage::DivisionByZero,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_ASSERT));
    // Check expected byte (0 = false)
    let assert_pos = bc.iter().position(|&b| b == OP_ASSERT).unwrap();
    assert_eq!(bc[assert_pos + 1], 0u8);
}

// ─── Storage Live/Dead are Nops ───

#[test]
fn storage_live_dead_nop() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::StorageLive(LocalIdx::from_raw(1)),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::StorageDead(LocalIdx::from_raw(1)),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
}

// ─── String Constant (Stub) ───

#[test]
fn unit_constant_stub() {
    let mut body = dummy_body();
    let dest = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::ERROR,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    // Should succeed via stub path
    assert!(result.is_ok());
}

// ─── Unreachable Terminator ───

#[test]
fn unreachable_terminator_nop() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Unreachable,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
}

// ─── Goto Terminator ───

#[test]
fn goto_terminator() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)].terminator = Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(5),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_JUMP));
    let jump_pos = bc.iter().position(|&b| b == OP_JUMP).unwrap();
    let target = u32::from_le_bytes([
        bc[jump_pos + 1],
        bc[jump_pos + 2],
        bc[jump_pos + 3],
        bc[jump_pos + 4],
    ]);
    assert_eq!(target, 5);
}

// ─── Nop Statement ───

#[test]
fn nop_statement() {
    let mut body = dummy_body();
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Nop,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body));
    assert!(result.is_ok());
}
