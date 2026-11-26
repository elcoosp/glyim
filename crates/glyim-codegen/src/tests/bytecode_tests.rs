//! Bytecode backend tests for unstubbing features.

use crate::{BytecodeBackend, CodegenBackend};
use glyim_core::{BinOp, CrateId, DefId, IndexVec, LocalDefId};
use glyim_mir::*;
use glyim_span::Span;
use glyim_type::{FieldIdx, Ty};
use std::sync::Arc;

// Opcode constants
use glyim_core::primitives::Mutability;
const OP_LOAD_LOCAL_ADDR: u8 = 0x29;
const OP_STORE_FIELD: u8 = 0x2A;
const OP_DROP: u8 = 0x2C;
const OP_ASSERT: u8 = 0x21;
const OP_LOAD_CONST: u8 = 0x01;
const OP_ADD: u8 = 0x02;

fn dummy_body(statements: Vec<Statement>, terminator: Terminator) -> Arc<Body> {
    let bb_data = BasicBlockData {
        statements,
        terminator,
        is_cleanup: false,
    };
    let mut blocks = IndexVec::new();
    blocks.push(bb_data);
    Arc::new(Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks: blocks,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    })
}

#[test]
fn assign_with_field_projection_emits_store_field() {
    let field_idx = FieldIdx::from_usize(0);
    let local_idx = LocalIdx::from_usize(0);
    let place = Place {
        local: local_idx,
        projection: vec![ProjectionElem::Field(field_idx)].into_boxed_slice(),
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

    // Build body with locals
    let mut b = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    b.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let mut block = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    block.statements.push(stmt);
    b.basic_blocks.push(block);
    let body: Arc<Body> = Arc::new(b);

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();

    // Verify bytecode contains OP_STORE_FIELD (terminator follows it)
    assert!(
        bytecode.contains(&OP_STORE_FIELD),
        "OP_STORE_FIELD missing in bytecode"
    );
}

#[test]
fn ref_with_projection_emits_addr_and_offset() {
    // Use field index 1 so offset = 8, forcing an OP_ADD for the offset.
    let field_idx = FieldIdx::from_usize(1);
    let local_idx = LocalIdx::from_usize(0);
    let place = Place {
        local: local_idx,
        projection: vec![ProjectionElem::Field(field_idx)].into_boxed_slice(),
    };
    let rvalue = Rvalue::Ref(place, BorrowKind::Shared);
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(LocalIdx::from_usize(1)), rvalue),
        source_info: SourceInfo::new(Span::DUMMY),
    };

    // Build body with locals so emit_place_address doesn't early-return
    let mut b = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    b.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    b.locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let mut block = BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    block.statements.push(stmt);
    b.basic_blocks.push(block);
    let body: Arc<Body> = Arc::new(b);

    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();

    let mut saw_load_addr = false;
    let mut saw_add = false;
    for &op in &bytecode {
        if op == OP_LOAD_LOCAL_ADDR {
            saw_load_addr = true;
        }
        if op == OP_ADD {
            saw_add = true;
        }
    }
    assert!(saw_load_addr, "OP_LOAD_LOCAL_ADDR missing");
    assert!(saw_add, "OP_ADD missing for offset");
}

#[test]
fn drop_terminator_emits_op_drop() {
    let place = Place::new(LocalIdx::from_usize(0));
    let term = TerminatorKind::Drop {
        place,
        target: BasicBlockIdx::from_usize(1),
        cleanup: None,
    };
    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: term,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let bb1 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut blocks = IndexVec::new();
    blocks.push(bb0);
    blocks.push(bb1);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks: blocks,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();
    assert!(
        bytecode.contains(&OP_DROP),
        "OP_DROP not emitted for Drop terminator"
    );
}

#[test]
fn assert_terminator_emits_op_assert() {
    let cond = Operand::Constant(MirConst {
        kind: MirConstKind::Bool(false),
        ty: Ty::BOOL,
        span: Span::DUMMY,
    });
    let term = TerminatorKind::Assert {
        cond,
        expected: true,
        target: BasicBlockIdx::from_usize(1),
        cleanup: None,
        msg: AssertMessage::Overflow(BinOp::Add),
    };
    let bb0 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: term,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let bb1 = BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    };
    let mut blocks = IndexVec::new();
    blocks.push(bb0);
    blocks.push(bb1);
    let body = Arc::new(Body {
        owner: DefId::new(CrateId::from_usize(0), LocalDefId::from_usize(0)),
        basic_blocks: blocks,
        locals: IndexVec::new(),
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();
    assert!(
        bytecode.contains(&OP_ASSERT),
        "OP_ASSERT not emitted for Assert terminator"
    );
}

#[test]
fn string_constant_emits_load_const() {
    let constant = MirConst {
        kind: MirConstKind::Int(123),
        ty: Ty::ERROR,
        span: Span::DUMMY,
    };
    let operand = Operand::Constant(constant);
    let place = Place::new(LocalIdx::from_usize(0));
    let stmt = Statement {
        kind: StatementKind::Assign(place, Rvalue::Use(operand)),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let body = dummy_body(
        vec![stmt],
        Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();
    let mut found_load_const = false;
    for &op in &bytecode {
        if op == OP_LOAD_CONST {
            found_load_const = true;
            break;
        }
    }
    assert!(found_load_const, "OP_LOAD_CONST not emitted for constant");
}

#[test]
fn fn_constant_emits_load_const() {
    let constant = MirConst {
        kind: MirConstKind::Int(456),
        ty: Ty::ERROR,
        span: Span::DUMMY,
    };
    let operand = Operand::Constant(constant);
    let place = Place::new(LocalIdx::from_usize(0));
    let stmt = Statement {
        kind: StatementKind::Assign(place, Rvalue::Use(operand)),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    let body = dummy_body(
        vec![stmt],
        Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
    );
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let bytecode = backend.generate_function(&body).unwrap();
    let mut found_load_const = false;
    for &op in &bytecode {
        if op == OP_LOAD_CONST {
            found_load_const = true;
            break;
        }
    }
    assert!(found_load_const, "OP_LOAD_CONST not emitted for constant");
}
