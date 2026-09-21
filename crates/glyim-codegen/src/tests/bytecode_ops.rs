//! Tests for bytecode backend: OP_INDEX, OP_FIELD, OP_REPEAT, and stub removal.
//!
//! Test cases:
//! - W5-C03-T01: arr[i] emits OP_LOAD_LOCAL_ADDR + offset + OP_DEREF
//! - W5-C03-T02: [value; count] emits OP_REPEAT
//! - W5-C03-T03: struct aggregate emits OP_AGGREGATE with field count
//! - W5-C03-T04: field access emits OP_LOAD_LOCAL_ADDR + offset + OP_ADD
//! - W5-C03-T05: out-of-bounds local panics (stub removed)
//! - W5-C03-T06: zero-sized element panics (stub removed)

use crate::{
    BytecodeBackend, CodegenBackend, LayoutProvider, OP_ADD, OP_AGGREGATE, OP_ASSERT, OP_DEREF,
    OP_LOAD_CONST, OP_LOAD_LOCAL, OP_LOAD_LOCAL_ADDR, OP_LT, OP_MUL, OP_REPEAT,
};
use glyim_core::primitives::Mutability;
use glyim_core::{CrateId, DefId, IndexVec, LocalDefId};
use glyim_mir::VariantIdx;
use glyim_mir::{
    AggregateKind, BasicBlockData, Body, BorrowKind, LocalDecl, LocalIdx, MirConst, MirConstKind,
    Operand, Place, ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use glyim_span::Span;
use glyim_type::{FieldIdx, Ty};
use std::sync::Arc;

/// Helper: create a `SourceInfo` with a dummy span.
fn src_info() -> SourceInfo {
    SourceInfo::new(Span::DUMMY)
}

/// Helper: create a `LocalDecl` with the given type and mutability.
fn local_decl(ty: Ty, mutability: Mutability) -> LocalDecl {
    LocalDecl {
        ty,
        mutability,
        source_info: src_info(),
    }
}

/// Helper: create a `Terminator` for `Return`.
fn return_terminator() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        source_info: src_info(),
    }
}

/// Helper: create a constant integer operand.
fn const_int_operand(val: i128) -> Operand {
    Operand::Constant(MirConst {
        kind: MirConstKind::Int(val),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    })
}

/// Helper: build a minimal MIR `Body` with the given locals, statements, and terminator.
fn build_body(
    locals: IndexVec<LocalIdx, LocalDecl>,
    statements: Vec<Statement>,
    terminator: Terminator,
) -> Arc<Body> {
    let mut block = BasicBlockData::new(terminator);
    block.statements = statements;

    let mut basic_blocks = IndexVec::new();
    basic_blocks.push(block);

    Arc::new(Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)),
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: Span::DUMMY,
        var_debug_info: vec![],
    })
}

/// Return the number of operand bytes consumed by an opcode.
/// For variable-length instructions (AGGREGATE, SWITCH_INT, CALL, REPEAT),
/// returns `None` since we cannot determine the size without parsing sub-operands.
fn opcode_operand_size(op: u8) -> Option<usize> {
    match op {
        0x01 => Some(8),        // OP_LOAD_CONST: i64
        0x02..=0x06 => Some(0), // OP_ADD..OP_REM
        0x07..=0x0F => Some(0), // comparison/logical/not/neg
        0x11..=0x15 => Some(0), // bitwise/shift
        0x16 => Some(4),        // OP_LOAD_LOCAL: u32
        0x17 => Some(4),        // OP_STORE_LOCAL: u32
        0x18 => Some(0),        // OP_RETURN
        0x19 => Some(4),        // OP_JUMP_IF: u32
        0x1A => Some(4),        // OP_JUMP: u32
        0x1C => Some(1),        // OP_CAST: u8 kind
        0x1E => Some(0),        // OP_DISCRIMINANT
        0x1F => Some(0),        // OP_LEN
        0x21 => Some(5),        // OP_ASSERT: u8 + u32
        0x29 => Some(4),        // OP_LOAD_LOCAL_ADDR: u32
        0x2A => Some(0),        // OP_STORE_FIELD
        0x2B => Some(0),        // OP_DEREF
        0x2C => Some(0),        // OP_DROP
        // Variable-length: cannot determine without parsing sub-operands
        0x1B | 0x22 | 0x1D | 0x20 | 0x2D => None,
        _ => Some(0), // Unknown opcode, assume no operands
    }
}

/// Disassemble bytecode into a sequence of (position, opcode) pairs,
/// properly skipping operand bytes for fixed-size instructions.
/// Stops when encountering a variable-length instruction.
fn disasm_opcodes(bc: &[u8]) -> Vec<(usize, u8)> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < bc.len() {
        let op = bc[pos];
        result.push((pos, op));
        pos += 1;
        match opcode_operand_size(op) {
            Some(skip) => pos += skip,
            None => break, // Variable-length instruction; stop parsing
        }
    }
    result
}

/// Find the position of the first occurrence of an opcode in the instruction stream,
/// properly accounting for instruction boundaries. Returns `None` if not found.
fn find_opcode(bc: &[u8], target: u8) -> Option<usize> {
    disasm_opcodes(bc)
        .into_iter()
        .find_map(|(pos, op)| if op == target { Some(pos) } else { None })
}

// ---------------------------------------------------------------------------
// W5-C03-T01: arr[i] emits OP_LOAD_LOCAL_ADDR + offset + OP_DEREF
// ---------------------------------------------------------------------------
#[test]
fn test_index_emits_load_addr_plus_offset_plus_deref() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::I32, Mutability::Not)); // local 0: return
    locals.push(local_decl(Ty::I32, Mutability::Mut)); // local 1: array
    locals.push(local_decl(Ty::I32, Mutability::Not)); // local 2: index

    // _0 = Copy(_1[_2])
    let indexed_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(2))]),
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(indexed_place)),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let bc = backend
        .generate_function(&body)
        .expect("generate_function should succeed");

    // Verify the bytecode contains the expected opcode sequence using
    // instruction-boundary-aware position finding:
    // OP_LOAD_LOCAL_ADDR (base of local 1)
    // OP_LOAD_LOCAL (index local 2)
    // OP_LOAD_CONST (element size)
    // OP_MUL (index * elem_size)
    // OP_ADD (base + offset)
    // OP_DEREF (dereference the computed address)
    let la = find_opcode(&bc, OP_LOAD_LOCAL_ADDR).expect("should contain OP_LOAD_LOCAL_ADDR");
    let ll = find_opcode(&bc, OP_LOAD_LOCAL).expect("should contain OP_LOAD_LOCAL");
    let lc = find_opcode(&bc, OP_LOAD_CONST).expect("should contain OP_LOAD_CONST");
    let m = find_opcode(&bc, OP_MUL).expect("should contain OP_MUL");
    let a = find_opcode(&bc, OP_ADD).expect("should contain OP_ADD");
    let d = find_opcode(&bc, OP_DEREF).expect("should contain OP_DEREF");

    assert!(
        la < ll,
        "OP_LOAD_LOCAL_ADDR at {} should precede OP_LOAD_LOCAL at {}",
        la,
        ll
    );
    assert!(
        ll < lc,
        "OP_LOAD_LOCAL at {} should precede OP_LOAD_CONST at {}",
        ll,
        lc
    );
    assert!(
        lc < m,
        "OP_LOAD_CONST at {} should precede OP_MUL at {}",
        lc,
        m
    );
    assert!(m < a, "OP_MUL at {} should precede OP_ADD at {}", m, a);
    assert!(a < d, "OP_ADD at {} should precede OP_DEREF at {}", a, d);
}

// ---------------------------------------------------------------------------
// W5-C03-T02: [value; count] emits OP_REPEAT
// ---------------------------------------------------------------------------
#[test]
fn test_repeat_emits_op_repeat() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 0: return

    // _0 = Repeat(Const(1), Const(5))
    let value = const_int_operand(1);
    let count = MirConst {
        kind: MirConstKind::Uint(5),
        ty: Ty::UNIT,
        span: Span::DUMMY,
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Repeat(value, count),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let bc = backend
        .generate_function(&body)
        .expect("generate_function should succeed");

    assert!(
        bc.contains(&OP_REPEAT),
        "bytecode should contain OP_REPEAT for [value; count]"
    );
}

// ---------------------------------------------------------------------------
// W5-C03-T03: Point { x: 1, y: 2 } emits OP_AGGREGATE with 2 fields
// ---------------------------------------------------------------------------
#[test]
fn test_aggregate_emits_op_aggregate_with_field_count() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 0: return

    // _0 = Aggregate(Tuple, [Const(1), Const(2)])
    let operands = vec![const_int_operand(1), const_int_operand(2)];
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Aggregate(AggregateKind::Tuple, operands),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let bc = backend
        .generate_function(&body)
        .expect("generate_function should succeed");

    // Find OP_AGGREGATE and verify the field count encoded as u32 LE
    let agg_pos = bc
        .iter()
        .position(|&b| b == OP_AGGREGATE)
        .expect("should contain OP_AGGREGATE");

    let count_start = agg_pos + 1;
    assert!(
        count_start + 4 <= bc.len(),
        "not enough bytes after OP_AGGREGATE for field count"
    );
    let field_count = u32::from_le_bytes([
        bc[count_start],
        bc[count_start + 1],
        bc[count_start + 2],
        bc[count_start + 3],
    ]);
    assert_eq!(field_count, 2, "OP_AGGREGATE should encode 2 fields");
}

// ---------------------------------------------------------------------------
// W5-C03-T04: field access emits OP_LOAD_LOCAL_ADDR + offset + OP_ADD
// ---------------------------------------------------------------------------
#[test]
fn test_field_access_emits_load_addr_plus_offset() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 0: return
    locals.push(local_decl(Ty::UNIT, Mutability::Mut)); // local 1: struct

    // _0 = Copy(_1.0)
    let field_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(field_place)),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let bc = backend
        .generate_function(&body)
        .expect("generate_function should succeed");

    // Verify: OP_LOAD_LOCAL_ADDR, then OP_LOAD_CONST (field offset), then OP_ADD, then OP_DEREF
    let la = find_opcode(&bc, OP_LOAD_LOCAL_ADDR).expect("should contain OP_LOAD_LOCAL_ADDR");
    let lc =
        find_opcode(&bc, OP_LOAD_CONST).expect("should contain OP_LOAD_CONST for field offset");
    let a = find_opcode(&bc, OP_ADD).expect("should contain OP_ADD for field offset addition");
    let d = find_opcode(&bc, OP_DEREF).expect("should contain OP_DEREF for field read");

    assert!(
        la < lc,
        "OP_LOAD_LOCAL_ADDR at {} should precede OP_LOAD_CONST at {}",
        la,
        lc
    );
    assert!(
        lc < a,
        "OP_LOAD_CONST at {} should precede OP_ADD at {}",
        lc,
        a
    );
    assert!(a < d, "OP_ADD at {} should precede OP_DEREF at {}", a, d);
}

// ---------------------------------------------------------------------------
// W5-C03-T05: out-of-bounds local panics (stub removed)
// ---------------------------------------------------------------------------
#[test]
#[should_panic(expected = "local index out of bounds")]
fn test_oob_local_panics() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 0: return
    // Intentionally no local 5 -- out of bounds

    // _0 = Ref(_5.*) -- _5 does not exist, projection non-empty so bounds check fires
    let oob_place = Place {
        local: LocalIdx::from_raw(5),
        projection: Box::new([ProjectionElem::Deref]),
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Ref(oob_place, BorrowKind::Shared),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let _ = backend.generate_function(&body);
}

/// Layout provider that always reports zero size, for testing the zero-sized
/// element panic path in `emit_place_address`.
struct ZeroSizeLayoutProvider;

impl LayoutProvider for ZeroSizeLayoutProvider {
    fn field_offset(&self, _ty: Ty, field_idx: FieldIdx) -> u64 {
        (field_idx.to_raw() as u64)
            .saturating_mul(8)
            .saturating_add(8)
    }

    fn size_of(&self, _ty: Ty) -> u64 {
        0
    }

    fn variant_type(&self, _enum_ty: Ty, _variant_idx: VariantIdx) -> Ty {
        Ty::ERROR
    }
    fn tag_offset(&self, _ty: Ty) -> u64 {
        0
    }
}

// ---------------------------------------------------------------------------
// W5-C03-T06: zero-sized element handled gracefully (stub removed)
// ---------------------------------------------------------------------------
#[test]
fn test_zero_sized_element_handled() {
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    )
    .with_layout_provider(Box::new(ZeroSizeLayoutProvider));

    let mut locals = IndexVec::new();
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 0: return
    locals.push(local_decl(Ty::UNIT, Mutability::Mut)); // local 1: array
    locals.push(local_decl(Ty::UNIT, Mutability::Not)); // local 2: index

    // _0 = Copy(_1[_2]) -- zero-sized elements should not panic, just yield 0 offset
    let indexed_place = Place {
        local: LocalIdx::from_raw(1),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(2))]),
    };
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(LocalIdx::from_raw(0)),
            Rvalue::Use(Operand::Copy(indexed_place)),
        ),
        source_info: src_info(),
    };

    let body = build_body(locals, vec![stmt], return_terminator());
    let result = backend.generate_function(&body);
    assert!(result.is_ok(), "Should handle ZST arrays without panicking");

    let bc = result.unwrap();
    // Should still emit OP_LOAD_LOCAL_ADDR and OP_DEREF, and an OP_ADD with 0.
    // §15.1: a ZST index must still *bounds-check* (panic on OOB), so the
    // bytecode must contain the index/len comparison + assert, not just add 0.
    assert!(find_opcode(&bc, OP_LOAD_LOCAL_ADDR).is_some());
    assert!(find_opcode(&bc, OP_ADD).is_some());
    assert!(find_opcode(&bc, OP_DEREF).is_some());
    assert!(
        find_opcode(&bc, OP_LT).is_some(),
        "ZST index must emit a bounds-check comparison (OP_LT)"
    );
    assert!(
        find_opcode(&bc, OP_ASSERT).is_some(),
        "ZST index must emit an OP_ASSERT so an out-of-range access traps"
    );
}
