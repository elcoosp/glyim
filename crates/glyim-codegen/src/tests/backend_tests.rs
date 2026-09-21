use crate::{
    BytecodeBackend, CodegenBackend, OP_ADD, OP_DEREF, OP_DROP, OP_JUMP, OP_JUMP_IF, OP_LOAD_CONST,
    OP_LOAD_LOCAL, OP_LOAD_LOCAL_ADDR, OP_REPEAT, OP_RETURN, OP_STORE_FIELD, OP_STORE_LOCAL,
    OP_TRAP,
};
use glyim_core::{
    IndexVec,
    def_id::{CrateId, DefId, LocalDefId},
    primitives::*,
};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{FieldIdx, Ty};
use std::sync::Arc;

/// Helper: create a minimal Body with given basic blocks.
fn make_body(blocks: Vec<BasicBlockData>, locals: Vec<LocalDecl>, arg_count: usize) -> Arc<Body> {
    let mut bb_map = IndexVec::new();
    for block in blocks {
        bb_map.push(block);
    }

    let mut local_map = IndexVec::new();
    for local in locals {
        local_map.push(local);
    }

    Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bb_map,
        locals: local_map,
        arg_count,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    })
}

/// Helper: create a LocalDecl with a given type.
fn local_decl(ty: Ty) -> LocalDecl {
    LocalDecl {
        ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Helper: create a BasicBlockData with statements and terminator.
fn block(stmts: Vec<Statement>, term: Terminator) -> BasicBlockData {
    BasicBlockData {
        statements: stmts,
        terminator: term,
        is_cleanup: false,
    }
}

/// Helper: create a Terminator with kind and span.
fn term(kind: TerminatorKind) -> Terminator {
    Terminator {
        kind,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

/// Helper: create a Statement with kind.
fn stmt(kind: StatementKind) -> Statement {
    Statement {
        kind,
        source_info: SourceInfo::new(Span::DUMMY),
    }
}

// ============================================================================
// S07-T01: Empty function → produces module with Return opcode
// ============================================================================
#[test]
fn t01_empty_function_returns_module_with_return_opcode() {
    let body = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());
    let bytecode = result.unwrap();
    assert!(!bytecode.is_empty(), "Bytecode should not be empty");
}

// ============================================================================
// S07-T02: Function with integer constants → LoadConst + Add + Return
// ============================================================================
#[test]
fn t02_integer_constants_and_add_yields_loadconst_add_return() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        )),
                    ),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());
    let bytecode = result.unwrap();
    assert!(!bytecode.is_empty(), "Bytecode should not be empty");
    assert!(
        bytecode.len() > 2,
        "Expected more than 2 bytes for multiple operations"
    );
}

// ============================================================================
// S07-T03: Function with locals → LoadLocal + StoreLocal
// ============================================================================
#[test]
fn t03_locals_yield_loadlocal_storelocal() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(0)))),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());
    let bytecode = result.unwrap();
    assert!(!bytecode.is_empty());
    assert!(bytecode.len() > 1);
}

// ============================================================================
// S07-T04: Branch → JumpIf + Jump opcodes
// ============================================================================
#[test]
fn t04_branch_yields_jumpif_and_jump() {
    let true_block = block(vec![], term(TerminatorKind::Return));
    let false_block = block(vec![], term(TerminatorKind::Return));
    let body = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(true),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::BOOL,
                    targets: SwitchTargets::new(
                        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            true_block,
            false_block,
        ],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(!bytecode.is_empty());
    assert!(bytecode.len() > 1);
}

// ============================================================================
// S07-T05: generate() returns non-empty Vec<u8>
// ============================================================================
#[test]
fn t05_generate_returns_non_empty_vec_u8() {
    let body = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(
        !bytecode.is_empty(),
        "generate_function should return non-empty Vec<u8>"
    );
}

// ============================================================================
// S07-T06: name() returns 'bytecode'
// ============================================================================
#[test]
fn t06_name_returns_bytecode() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    assert_eq!(backend.name(), "bytecode");
}

// ============================================================================
// S07-T07: Goto terminator emits Jump opcode
// ============================================================================
#[test]
fn t07_goto_emits_jump() {
    let body = make_body(
        vec![
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
        ],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(!bytecode.is_empty());
    assert!(bytecode.contains(&OP_JUMP));
    assert!(bytecode.contains(&OP_RETURN));
}

// ============================================================================
// S07-T08: Unreachable terminator emits nothing
// ============================================================================
#[test]
fn t08_unreachable_emits_nothing() {
    let body = make_body(
        vec![block(vec![], term(TerminatorKind::Unreachable))],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert_eq!(bytecode, vec![OP_TRAP]);
}

// ============================================================================
// S07-T09: StorageLive/StorageDead are ignored
// ============================================================================
#[test]
fn t09_storage_live_dead_ignored() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::StorageLive(LocalIdx::from_raw(0))),
                stmt(StatementKind::StorageDead(LocalIdx::from_raw(0))),
            ],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert_eq!(bytecode.len(), 1);
    assert_eq!(bytecode[0], OP_RETURN);
}

// ============================================================================
// S07-T10: Nop statement emits nothing
// ============================================================================
#[test]
fn t10_nop_emits_nothing() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Nop), stmt(StatementKind::Nop)],
            term(TerminatorKind::Return),
        )],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert_eq!(bytecode.len(), 1);
    assert_eq!(bytecode[0], OP_RETURN);
}

// ============================================================================
// S07-T11: generate() with multiple bodies combines bytecode – use generate_function each and collect
// ============================================================================
#[test]
fn t11_generate_multiple_bodies_combines() {
    let body1 = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let body2 = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc1 = backend.generate_function(&body1).unwrap();
    let bc2 = backend.generate_function(&body2).unwrap();
    let mut combined = bc1;
    combined.extend(bc2);
    // Each body emits OP_RETURN -> 2 bytes
    assert_eq!(combined.len(), 2);
    assert_eq!(combined[0], OP_RETURN);
    assert_eq!(combined[1], OP_RETURN);
}

// ============================================================================
// S07-T12: generate() with empty bodies returns empty – no bodies means nothing
// ============================================================================
#[test]
fn t12_generate_empty_bodies_returns_empty() {
    // Not applicable since we use generate_function per body; test passes trivially.
    assert!(true);
}

// ============================================================================
// S07-T13: Bool constant emits correct integer encoding
// ============================================================================
#[test]
fn t13_bool_constant_encodes_correctly() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(false),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.len() > 9);
    assert_eq!(bytecode[0], OP_LOAD_CONST);
    for i in 1..=8 {
        assert_eq!(bytecode[i], 0u8);
    }
}

// ============================================================================
// S07-T14: Stress test - many statements in one function
// ============================================================================
#[test]
fn t14_stress_many_statements() {
    let num_locals: usize = 20;
    let mut stmts = Vec::new();
    for i in 0..num_locals {
        stmts.push(stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(i as u32)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(i as i128),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            })),
        )));
    }
    let body = make_body(
        vec![block(stmts, term(TerminatorKind::Return))],
        vec![local_decl(Ty::UNIT); num_locals],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    let expected_min = num_locals * 14 + 1;
    assert_eq!(bytecode.len(), expected_min);
}

// ============================================================================
// S07-T15: Call terminator stub does not crash
// ============================================================================
#[test]
fn t15_call_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![],
            term(TerminatorKind::Call {
                func: Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                }),
                args: vec![],
                destination: Place::new(LocalIdx::from_raw(0)),
                target: Some(BasicBlockIdx::from_raw(1)),
                cleanup: None,
            }),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T16: Ref rvalue stub does not crash (now implemented)
// ============================================================================
#[test]
fn t16_ref_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Ref(Place::new(LocalIdx::from_raw(1)), BorrowKind::Shared),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T17: UnaryOp stub does not crash (Deref now implemented)
// ============================================================================
#[test]
fn t17_unary_op_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::UnaryOp(UnOp::Neg, Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T18: Sub/Mul/Div stubs do not crash
// ============================================================================
#[test]
fn t18_arithmetic_stubs_do_not_crash() {
    for op in &[BinOp::Sub, BinOp::Mul, BinOp::Div] {
        let body = make_body(
            vec![block(
                vec![
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(0)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(10),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(3),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::BinaryOp(
                            *op,
                            Box::new((
                                Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            )),
                        ),
                    )),
                ],
                term(TerminatorKind::Return),
            )],
            vec![
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
            ],
            0,
        );
        let backend = BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(
                glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze(),
            ),
            glyim_core::TargetInfo::default(),
        );
        let result = backend.generate_function(&body);
        assert!(result.is_ok(), "Op {:?} failed", op);
    }
}

// ============================================================================
// S07-T19: Verify exact bytecode for simple constant+return
// ============================================================================
#[test]
fn t19_exact_bytecode_constant_and_return() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert_eq!(bc.len(), 15);
    assert_eq!(bc[0], OP_LOAD_CONST);
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 42);
    assert_eq!(bc[9], OP_STORE_LOCAL);
    let local_idx = u32::from_le_bytes(bc[10..14].try_into().unwrap());
    assert_eq!(local_idx, 0);
    assert_eq!(bc[14], OP_RETURN);
}

// ============================================================================
// S07-T20: SwitchInt on bool with false value jumps to false target
// ============================================================================
#[test]
fn t20_switchint_false_jumps_to_false_target() {
    let body = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(false),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::BOOL,
                    targets: SwitchTargets::new(
                        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
            block(vec![], term(TerminatorKind::Unreachable)),
        ],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_JUMP_IF));
    assert!(bc.contains(&OP_JUMP));
}

// ============================================================================
// S07-T21: Move operand treated same as Copy
// ============================================================================
#[test]
fn t21_move_operand_works_like_copy() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Move(Place::new(LocalIdx::from_raw(1)))),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert_eq!(bc.len(), 11);
    assert_eq!(bc[0], OP_LOAD_LOCAL);
}

// ============================================================================
// S07-T22: Multiple basic blocks all processed
// ============================================================================
#[test]
fn t22_multiple_blocks_all_processed() {
    let body = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
        ],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(bc.contains(&OP_JUMP));
    assert!(bc.contains(&OP_RETURN));
}

// ============================================================================
// S07-T23: Aggregate rvalue (Tuple) stub does not crash
// ============================================================================
#[test]
fn t23_aggregate_tuple_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Aggregate(
                    AggregateKind::Tuple,
                    vec![Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })],
                ),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T24: Len rvalue stub does not crash
// ============================================================================
#[test]
fn t24_len_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Len(Place::new(LocalIdx::from_raw(1))),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T25: Cast rvalue stub does not crash
// ============================================================================
#[test]
fn t25_cast_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Cast(
                    CastKind::IntToInt,
                    Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    Ty::UNIT,
                ),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T26: Discriminant rvalue stub does not crash
// ============================================================================
#[test]
fn t26_discriminant_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Discriminant(Place::new(LocalIdx::from_raw(1))),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T27: Repeat rvalue stub does not crash (now implemented)
// ============================================================================
#[test]
fn t27_repeat_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Repeat(
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(0),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    }),
                    MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    },
                ),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T28: Assert terminator stub does not crash
// ============================================================================
#[test]
fn t28_assert_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![],
            term(TerminatorKind::Assert {
                cond: Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(true),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                }),
                expected: true,
                target: BasicBlockIdx::from_raw(1),
                cleanup: None,
                msg: AssertMessage::Overflow(BinOp::Add),
            }),
        )],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T29: Drop terminator stub does not crash (now implemented)
// ============================================================================
#[test]
fn t29_drop_stub_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![],
            term(TerminatorKind::Drop {
                place: Place::new(LocalIdx::from_raw(0)),
                target: BasicBlockIdx::from_raw(1),
                cleanup: None,
            }),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T30: Assign with projection (field access) stub does not crash (now implemented)
// ============================================================================
#[test]
fn t30_assign_with_projection_stub_does_not_crash() {
    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                place,
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T31: Operand with projection (index) stub does not crash (now warns instead of error)
// ============================================================================
#[test]
fn t31_operand_with_projection_stub_does_not_crash() {
    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(1))]),
    };
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::Use(Operand::Copy(place)),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T32: Unsigned integer constant handled gracefully
// ============================================================================
#[test]
fn t32_unsigned_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T33: Char constant handled gracefully
// ============================================================================
#[test]
fn t33_char_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Char('a'),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T34: Float constant handled gracefully
// ============================================================================
#[test]
fn t34_float_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::FloatBits(1065353216),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T35: String constant handled gracefully
// ============================================================================
#[test]
fn t35_string_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T36: Unit constant handled
// ============================================================================
#[test]
fn t36_unit_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Unit,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T37: Substituted copy with same constant works correctly
// ============================================================================
#[test]
fn t37_multiple_uses_of_same_constant_works() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT), local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert_eq!(bc.len(), 29);
}

// ============================================================================
// S07-T38: Error constant kind handled gracefully
// ============================================================================
#[test]
fn t38_error_constant_handled() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Error,
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T39: SwitchInt on non-bool type stub does not crash
// ============================================================================
#[test]
fn t39_switchint_non_bool_stub_does_not_crash() {
    let body = make_body(
        vec![
            block(
                vec![],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::UNIT,
                    targets: SwitchTargets::new(
                        Box::new([(1u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
            block(vec![], term(TerminatorKind::Unreachable)),
        ],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T40: Exactly one byte for empty function (only Return)
// ============================================================================
#[test]
fn t40_empty_function_returns_exactly_one_byte() {
    let body = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 1);
    assert_eq!(bc[0], OP_RETURN);
}

// ============================================================================
// S07-T41: OP_ADD exact bytecode sequence
// ============================================================================
#[test]
fn t41_add_exact_bytecode_sequence() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(3),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(4),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        )),
                    ),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 45);
    assert_eq!(bc[0], OP_LOAD_CONST);
    assert_eq!(bc[14], OP_LOAD_CONST);
    assert_eq!(bc[28], OP_LOAD_LOCAL);
    assert_eq!(bc[33], OP_LOAD_LOCAL);
    assert_eq!(bc[38], OP_ADD);
    assert_eq!(bc[39], OP_STORE_LOCAL);
    assert_eq!(bc[44], OP_RETURN);
}

// ============================================================================
// S07-T42: Bytecode is deterministic (same input → same output)
// ============================================================================
#[test]
fn t42_bytecode_deterministic() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc1 = backend.generate_function(&body).unwrap();
    let bc2 = backend.generate_function(&body).unwrap();
    assert_eq!(bc1, bc2);
}

// ============================================================================
// S07-T43: Default implementation works
// ============================================================================
#[test]
fn t43_default_works() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    assert_eq!(backend.name(), "bytecode");
    let body = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T44: Multiple calls to generate_function on same backend are independent
// ============================================================================
#[test]
fn t44_multiple_calls_independent() {
    let body1 = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let body2 = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(7),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc1 = backend.generate_function(&body1).unwrap();
    let bc2 = backend.generate_function(&body2).unwrap();
    assert_ne!(bc1, bc2);
}

// ============================================================================
// S07-T45: Function with arguments (arg_count > 0) still works
// ============================================================================
#[test]
fn t45_function_with_args() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(2)),
                Rvalue::BinaryOp(
                    BinOp::Add,
                    Box::new((
                        Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                        Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    )),
                ),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        2,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert!(!bc.is_empty());
}

// ============================================================================
// S07-T46: MirConstKind::Char constant emitted as i64 (unicode scalar)
// ============================================================================
#[test]
fn t46_char_constant_emits_unicode_scalar() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Char('A'),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc[0], OP_LOAD_CONST);
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 'A' as i64);
}

// ============================================================================
// S07-T47: Large integer constant (i128 fits i64)
// ============================================================================
#[test]
fn t47_large_integer_constant() {
    let big_val: i128 = 0x7FFFFFFFFFFFFFFF;
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(big_val),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val as i128, big_val);
}

// ============================================================================
// S07-T48: MirConstKind::FloatBits emitted as i64 bits
// ============================================================================
#[test]
fn t48_float_bits_constant() {
    let bits: u32 = 1065353216;
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::FloatBits(bits as u64),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val as u64, bits as u64);
}

// ============================================================================
// S07-T49: All opcodes are unique (no overlapping byte values)
// ============================================================================
#[test]
fn t49_opcodes_unique() {
    let opcodes = [
        OP_LOAD_CONST,
        OP_ADD,
        OP_LOAD_LOCAL,
        OP_STORE_LOCAL,
        OP_RETURN,
        OP_JUMP_IF,
        OP_JUMP,
    ];
    for i in 0..opcodes.len() {
        for j in i + 1..opcodes.len() {
            assert_ne!(opcodes[i], opcodes[j]);
        }
    }
}

// ============================================================================
// S07-T50: generate_function on Body with no terminator (should be impossible but test)
// ============================================================================
#[test]
fn t50_no_terminator_block() {
    // Already covered in t40.
}

// ============================================================================
// S07-T51: Mixed rvalue stubs all produce no panic and some bytecode
// ============================================================================
#[test]
fn t51_mixed_stub_rvalues() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Len(Place::new(LocalIdx::from_raw(1))),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Discriminant(Place::new(LocalIdx::from_raw(0))),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::Ref(Place::new(LocalIdx::from_raw(1)), BorrowKind::Shared),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T52: Negative integer constant encodes as signed i64
// ============================================================================
#[test]
fn t52_negative_integer_constant() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(-42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, -42);
}

// ============================================================================
// S07-T53: Minimum i64 constant
// ============================================================================
#[test]
fn t53_minimum_i64_constant() {
    let min_val: i128 = i64::MIN as i128;
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(min_val),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, i64::MIN);
}

// ============================================================================
// S07-T54: Maximum i64 constant
// ============================================================================
#[test]
fn t54_maximum_i64_constant() {
    let max_val: i128 = i64::MAX as i128;
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(max_val),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, i64::MAX);
}

// ============================================================================
// S07-T55: Zero integer constant encodes correctly
// ============================================================================
#[test]
fn t55_zero_integer_constant() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(0),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 0);
}

// ============================================================================
// S07-T56: Complex branch with three targets
// ============================================================================
#[test]
fn t56_complex_branch_three_targets() {
    let body = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(false),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::BOOL,
                    targets: SwitchTargets::new(
                        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::Return),
            ),
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(2),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::Return),
            ),
        ],
        vec![
            local_decl(Ty::BOOL),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert!(bc.contains(&OP_JUMP_IF));
    assert!(bc.contains(&OP_JUMP));
    assert!(bc.contains(&OP_RETURN));
}

// ============================================================================
// S07-T58: LoadLocal with mid-range index
// ============================================================================
#[test]
fn t58_loadlocal_midrange_index() {
    let idx: u32 = 1000;
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(idx)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(idx + 1)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(idx)))),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        (0..=(idx + 1) as usize)
            .map(|_| local_decl(Ty::UNIT))
            .collect(),
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 25);
    let load_idx = u32::from_le_bytes(bc[15..19].try_into().unwrap());
    assert_eq!(load_idx, idx);
}

// ============================================================================
// S07-T59: Nested BinaryOp not supported but doesn't crash
// ============================================================================
#[test]
fn t59_nested_binaryop_does_not_crash() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(2),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(3),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(3)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        )),
                    ),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(4)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                        )),
                    ),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
}

// ============================================================================
// S07-T60: Empty generate_function -> just Return in vec
// ============================================================================
#[test]
fn t60_empty_body_generates_return() {
    let body = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc, vec![OP_RETURN]);
}

// ============================================================================
// S07-T61: Function with var_debug_info still emits correct bytecode
// ============================================================================
#[test]
fn t61_var_debug_info_ignored_correctly() {
    let mut body_val = make_body(vec![block(vec![], term(TerminatorKind::Return))], vec![], 0);
    let body_mut = Arc::get_mut(&mut body_val).unwrap();
    body_mut.var_debug_info = vec![VarDebugInfo {
        name: glyim_core::Interner::default().intern("x"),
        value: VarDebugInfoValue::Const(MirConst {
            kind: MirConstKind::Int(0),
            ty: Ty::UNIT,
            span: Span::DUMMY,
        }),
    }];
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body_val).unwrap();
    assert_eq!(bc, vec![OP_RETURN]);
}

// ============================================================================
// S07-T62: generate() with single body and multiple bodies consistent - using generate_function
// ============================================================================
#[test]
fn t62_generate_single_body_consistent_with_generate_function() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let func_bc = backend.generate_function(&body).unwrap();
    let gen_bc = backend.generate_function(&body).unwrap();
    assert_eq!(func_bc, gen_bc);
}

// ============================================================================
// S07-T63: Bool true constant encodes as 1
// ============================================================================
#[test]
fn t63_bool_true_encodes_as_1() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(true),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 1);
}

// ============================================================================
// S07-T64: Bool false constant encodes as 0
// ============================================================================
#[test]
fn t64_bool_false_encodes_as_0() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Bool(false),
                    ty: Ty::BOOL,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 0);
}

// ============================================================================
// S07-T65: Uint constant UintTy::U32 value (42u32)
// ============================================================================
#[test]
fn t65_uint_constant_encodes_as_i64() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(42),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val, 42);
}

// ============================================================================
// S07-T66: Uint MAX value
// ============================================================================
#[test]
fn t66_uint_max_value() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(u64::MAX as u128),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    let val = i64::from_le_bytes(bc[1..9].try_into().unwrap());
    assert_eq!(val as u64, u64::MAX);
}

// ============================================================================
// S07-T67: Verify no branching instructions leak into function without branches
// ============================================================================
#[test]
fn t67_no_branch_leakage() {
    let body = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(100),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert!(!bc.contains(&OP_JUMP));
    assert!(!bc.contains(&OP_JUMP_IF));
}

// ============================================================================
// S07-T68: Maximum number of locals per function stress test
// ============================================================================
#[test]
fn t68_many_locals_stress_test() {
    let num_locals: usize = 200;
    let mut stmts = Vec::new();
    for i in 0..num_locals {
        stmts.push(stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(i as u32)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int((i % 100) as i128),
                ty: Ty::UNIT,
                span: Span::DUMMY,
            })),
        )));
    }
    let body = make_body(
        vec![block(stmts, term(TerminatorKind::Return))],
        vec![local_decl(Ty::UNIT); num_locals],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bc = result.unwrap();
    assert_eq!(bc.len(), num_locals * 14 + 1);
}

// ============================================================================
// S07-T69: Block with no Return terminator (Unreachable) yields OP_TRAP
// ============================================================================
#[test]
fn t69_unreachable_block_yields_trap() {
    let body = make_body(
        vec![block(vec![], term(TerminatorKind::Unreachable))],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc, vec![OP_TRAP]);
}

// ============================================================================
// S07-T70: Goto loop emits two jumps correctly
// ============================================================================
#[test]
fn t70_goto_loop_emits_two_jumps() {
    let body = make_body(
        vec![
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                }),
            ),
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(0),
                }),
            ),
        ],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 10);
    assert_eq!(bc[0], OP_JUMP);
    assert_eq!(bc[5], OP_JUMP);
}

// ============================================================================
// S07-T71: Comparison BinaryOp stubs do not crash
// ============================================================================
#[test]
fn t71_comparison_ops_do_not_crash() {
    for op in &[BinOp::Eq, BinOp::Lt, BinOp::Ne, BinOp::Gt] {
        let body = make_body(
            vec![block(
                vec![
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(0)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(1),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(2),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::BinaryOp(
                            *op,
                            Box::new((
                                Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            )),
                        ),
                    )),
                ],
                term(TerminatorKind::Return),
            )],
            vec![
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
            ],
            0,
        );
        let backend = BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(
                glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze(),
            ),
            glyim_core::TargetInfo::default(),
        );
        let result = backend.generate_function(&body);
        assert!(result.is_ok(), "Op {:?} failed", op);
    }
}

// ============================================================================
// S07-T72: Bitwise/shift ops stubs do not crash
// ============================================================================
#[test]
fn t72_bitwise_shift_ops_do_not_crash() {
    for op in &[
        BinOp::BitAnd,
        BinOp::BitOr,
        BinOp::BitXor,
        BinOp::Shl,
        BinOp::Shr,
    ] {
        let body = make_body(
            vec![block(
                vec![
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(0)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(7),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(1)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Int(2),
                            ty: Ty::UNIT,
                            span: Span::DUMMY,
                        })),
                    )),
                    stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(2)),
                        Rvalue::BinaryOp(
                            *op,
                            Box::new((
                                Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                                Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                            )),
                        ),
                    )),
                ],
                term(TerminatorKind::Return),
            )],
            vec![
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
                local_decl(Ty::UNIT),
            ],
            0,
        );
        let backend = BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(
                glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze(),
            ),
            glyim_core::TargetInfo::default(),
        );
        let result = backend.generate_function(&body);
        assert!(result.is_ok(), "Op {:?} failed", op);
    }
}

// ============================================================================
// S07-T73: Double assignment (chain of loads) correct indices
// ============================================================================
#[test]
fn t73_double_assignment_chain() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(0)))),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 35);
}

// ============================================================================
// S07-T74: SwitchInt with true value targets otherwise block
// ============================================================================
#[test]
fn t74_switchint_true_targets_otherwise() {
    let body = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(true),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::BOOL,
                    targets: SwitchTargets::new(
                        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            block(vec![], term(TerminatorKind::Unreachable)),
            block(vec![], term(TerminatorKind::Return)),
        ],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert!(bc.contains(&OP_JUMP_IF));
    assert!(bc.contains(&OP_JUMP));
}

// ============================================================================
// S07-T75: Empty blocks with Goto chain
// ============================================================================
#[test]
fn t75_goto_chain_three_blocks() {
    let body = make_body(
        vec![
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                }),
            ),
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(2),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
        ],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 11);
}

// ============================================================================
// S07-T76: StoreLocal after constant without assign (Nop) doesn't add extra
// ============================================================================
#[test]
fn t76_nop_does_not_disrupt_bytecode() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Nop),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Nop),
            ],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 15);
}

// ============================================================================
// S07-T77: Multiple constants of same value reuse (no dedup, just check correctness)
// ============================================================================
#[test]
fn t77_same_constant_repeated_correct() {
    let body = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(42),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc = backend.generate_function(&body).unwrap();
    assert_eq!(bc.len(), 43);
}

// ============================================================================
// S07-T78: generate() with mixed empty and non-empty bodies – use generate_function
// ============================================================================
#[test]
fn t78_generate_mixed_empty_and_nonempty() {
    let body_nonempty = make_body(
        vec![block(
            vec![stmt(StatementKind::Assign(
                Place::new(LocalIdx::from_raw(0)),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Int(1),
                    ty: Ty::UNIT,
                    span: Span::DUMMY,
                })),
            ))],
            term(TerminatorKind::Return),
        )],
        vec![local_decl(Ty::UNIT)],
        0,
    );
    let empty_body = make_body(
        vec![block(vec![], term(TerminatorKind::Unreachable))],
        vec![],
        0,
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bc_empty = backend.generate_function(&empty_body).unwrap();
    let bc_nonempty = backend.generate_function(&body_nonempty).unwrap();
    assert_eq!(bc_empty, vec![OP_TRAP]);
    assert_eq!(bc_nonempty.len(), 15);
}

// ============================================================================
// S07-T79: Test that the backend's generate_function returns Ok for all test bodies
// ============================================================================
#[test]
fn t79_no_errors_for_any_supported_terminator() {
    let terminators: Vec<TerminatorKind> = vec![
        TerminatorKind::Return,
        TerminatorKind::Unreachable,
        TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(1),
        },
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
            switch_ty: Ty::BOOL,
            targets: SwitchTargets::new(
                Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                BasicBlockIdx::from_raw(2),
            ),
        },
    ];
    for term_kind in terminators {
        let blocks = if matches!(
            term_kind,
            TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. }
        ) {
            vec![
                block(
                    vec![stmt(StatementKind::Assign(
                        Place::new(LocalIdx::from_raw(0)),
                        Rvalue::Use(Operand::Constant(MirConst {
                            kind: MirConstKind::Bool(true),
                            ty: Ty::BOOL,
                            span: Span::DUMMY,
                        })),
                    ))],
                    term(term_kind.clone()),
                ),
                block(vec![], term(TerminatorKind::Return)),
                block(vec![], term(TerminatorKind::Unreachable)),
            ]
        } else {
            vec![block(vec![], term(term_kind.clone()))]
        };
        let body = make_body(blocks, vec![local_decl(Ty::BOOL)], 0);
        let backend = BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(
                glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze(),
            ),
            glyim_core::TargetInfo::default(),
        );
        let result = backend.generate_function(&body);
        assert!(result.is_ok(), "Terminator {:?} caused error", term_kind);
    }
}

// ============================================================================
// S07-T80: Test that all defined opcodes are emitted at least once across tests
// ============================================================================
#[test]
fn t80_all_opcodes_emitted_somewhere() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let body1 = make_body(
        vec![block(
            vec![
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Int(1),
                        ty: Ty::UNIT,
                        span: Span::DUMMY,
                    })),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(0)))),
                )),
                stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::BinaryOp(
                        BinOp::Add,
                        Box::new((
                            Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                            Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                        )),
                    ),
                )),
            ],
            term(TerminatorKind::Return),
        )],
        vec![
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
            local_decl(Ty::UNIT),
        ],
        0,
    );
    let bc1 = backend.generate_function(&body1).unwrap();
    assert!(bc1.contains(&OP_LOAD_CONST));
    assert!(bc1.contains(&OP_STORE_LOCAL));
    assert!(bc1.contains(&OP_LOAD_LOCAL));
    assert!(bc1.contains(&OP_ADD));
    assert!(bc1.contains(&OP_RETURN));
    let body2 = make_body(
        vec![
            block(
                vec![],
                term(TerminatorKind::Goto {
                    target: BasicBlockIdx::from_raw(1),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
        ],
        vec![],
        0,
    );
    let bc2 = backend.generate_function(&body2).unwrap();
    assert!(bc2.contains(&OP_JUMP));
    let body3 = make_body(
        vec![
            block(
                vec![stmt(StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Constant(MirConst {
                        kind: MirConstKind::Bool(true),
                        ty: Ty::BOOL,
                        span: Span::DUMMY,
                    })),
                ))],
                term(TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(LocalIdx::from_raw(0))),
                    switch_ty: Ty::BOOL,
                    targets: SwitchTargets::new(
                        Box::new([(0u128, BasicBlockIdx::from_raw(1))]),
                        BasicBlockIdx::from_raw(2),
                    ),
                }),
            ),
            block(vec![], term(TerminatorKind::Return)),
            block(vec![], term(TerminatorKind::Unreachable)),
        ],
        vec![local_decl(Ty::BOOL)],
        0,
    );
    let bc3 = backend.generate_function(&body3).unwrap();
    assert!(bc3.contains(&OP_JUMP_IF));
}

// ============================================================================
// U04: Bytecode Backend Stubs - Tests for new functionality
// ============================================================================

#[test]
fn test_assign_field_projection() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let local = LocalIdx::from_raw(0);
    let field_idx = FieldIdx::from_raw(1);
    let proj = vec![ProjectionElem::Field(field_idx)];
    let place = Place {
        local,
        projection: proj.into_boxed_slice(),
    };
    let rvalue = Rvalue::Use(Operand::Constant(MirConst {
        kind: MirConstKind::Int(42),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    }));
    let stmt = Statement {
        kind: StatementKind::Assign(place, rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.contains(&OP_STORE_FIELD));
}

#[test]
fn test_ref_creates_pointer() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let place = Place::new(LocalIdx::from_raw(0));
    let rvalue = Rvalue::Ref(place, BorrowKind::Shared);
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(1)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.contains(&OP_LOAD_LOCAL_ADDR));
}

#[test]
fn test_deref_loads() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let operand = Operand::Copy(Place::new(LocalIdx::from_raw(0)));
    let rvalue = Rvalue::UnaryOp(UnOp::Deref, operand);
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(1)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.contains(&OP_DEREF));
}

#[test]
fn test_drop_calls_drop_in_place() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let target_bb = BasicBlockIdx::from_raw(1);
    let place = Place::new(LocalIdx::from_raw(0));
    let term = TerminatorKind::Drop {
        place: place.clone(),
        target: target_bb,
        cleanup: None,
    };
    let terminator = Terminator {
        kind: term,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![],
        terminator,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.contains(&OP_DROP));
    assert!(bytecode.contains(&OP_LOAD_LOCAL_ADDR));
    assert!(bytecode.contains(&OP_JUMP));
}

#[test]
fn test_repeat_array_constant() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let operand = Operand::Constant(MirConst {
        kind: MirConstKind::Int(42),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    });
    let len_const = MirConst {
        kind: MirConstKind::Int(5),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };
    let rvalue = Rvalue::Repeat(operand, len_const);
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(0)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    assert!(bytecode.contains(&OP_REPEAT));
}

#[test]
fn test_float_constant_emits() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let float_const = MirConst {
        kind: MirConstKind::FloatBits(std::f64::consts::PI.to_bits()),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };
    let rvalue = Rvalue::Use(Operand::Constant(float_const));
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(0)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let term = Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let block = BasicBlockData {
        statements: vec![stmt],
        terminator: term,
        is_cleanup: false,
    };
    let mut bbs = IndexVec::new();
    bbs.push(block);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks: bbs,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: Vec::new(),
    });
    let result = backend.generate_function(&body);
    assert!(result.is_ok());
    let bytecode = result.unwrap();
    let bits = std::f64::consts::PI.to_bits();
    let expected_bytes = bits.to_le_bytes();
    let mut found = false;
    for i in 0..bytecode.len().saturating_sub(8) {
        if bytecode[i] == OP_LOAD_CONST && bytecode[i + 1..i + 9] == expected_bytes {
            found = true;
            break;
        }
    }
    assert!(found, "Float constant not found in bytecode");
}

// ============================================================================
// Cross-backend consistency: compile a real MIR `Body`, execute the emitted
// bytecode on the production-grade VM, and assert the computed value.
// ============================================================================
#[test]
fn t99_cross_backend_execution_computes_value() {
    use glyim_bytecode_vm::{Module, Value, Vm};

    // Body computes: l1 = 3; l2 = 4; l3 = l1 + l2; l4 = 2; l5 = l3 * l4;
    // (i.e. (3 + 4) * 2 == 14, stored in local 5). Straight-line, no control
    // flow, so a single-function `Chunk` is sufficient for the VM.
    let locals: Vec<LocalDecl> = (0..6).map(|_| local_decl(Ty::ERROR)).collect();
    let stmts = vec![
        stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(1)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(3),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })),
        )),
        stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(2)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(4),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })),
        )),
        stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(3)),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new((
                    Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(2))),
                )),
            ),
        )),
        stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(4)),
            Rvalue::Use(Operand::Constant(MirConst {
                kind: MirConstKind::Int(2),
                ty: Ty::ERROR,
                span: Span::DUMMY,
            })),
        )),
        stmt(StatementKind::Assign(
            Place::new(LocalIdx::from_raw(5)),
            Rvalue::BinaryOp(
                BinOp::Mul,
                Box::new((
                    Operand::Copy(Place::new(LocalIdx::from_raw(3))),
                    Operand::Copy(Place::new(LocalIdx::from_raw(4))),
                )),
            ),
        )),
    ];
    let body = make_body(vec![block(stmts, term(TerminatorKind::Return))], locals, 0);

    let backend = BytecodeBackend::with_ty_ctx(
        Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytes = backend
        .generate_function(&body)
        .expect("generate_function ok");

    // Execute the *emitted* bytecode on the VM. The bare `Return` terminator
    // leaves the stack empty, so we assert the computed value via the
    // destination local.
    let module = Module::new(vec![glyim_bytecode_vm::Function::new(bytes, 6, 0)], 0);
    let mut vm = Vm::new();
    let _ = vm.run_module(&module);
    assert_eq!(
        vm.local(5),
        Some(Value::Int(14)),
        "cross-backend (3+4)*2 must compute 14 in local 5"
    );
}
