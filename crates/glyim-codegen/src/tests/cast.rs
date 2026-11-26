use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

#[test]
fn emit_cast_int_to_float() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let dest = LocalIdx::from_usize(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let target_ty = Ty::F64;
    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Cast(
                    CastKind::IntToFloat,
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(10),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    target_ty,
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_CAST));
    // The cast operand is two bytes: the cast kind, then the target type the
    // VM must convert to (the old single-byte form made `OP_CAST` a no-op in
    // the VM). `Int(10)` is `OP_LOAD_CONST` + 8 bytes, so the cast starts at
    // byte 9.
    assert_eq!(
        &result[9..12],
        &[OP_CAST, 2, glyim_bytecode_vm::CastTarget::F64 as u8],
        "IntToFloat over an f64 target: kind 2, target F64"
    );
}

#[test]
fn emit_cast_float_to_int() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let dest = LocalIdx::from_usize(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Cast(
                    CastKind::FloatToInt,
                    Operand::Constant(MirConst {
                        kind: MirConstKind::FloatBits(1.0f64.to_bits()),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    Ty::ERROR,
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_CAST));
    // `FloatBits(1.0)` travels under `OP_LOAD_CONST_F64` + 8 bytes, so the
    // cast starts at byte 9. The target is `Ty::ERROR` (not a primitive),
    // which maps to the identity fallback `CastTarget::Unknown`.
    assert_eq!(
        &result[9..12],
        &[OP_CAST, 1, glyim_bytecode_vm::CastTarget::Unknown as u8],
        "FloatToInt over an unresolvable target: kind 1, target Unknown"
    );
}

#[test]
fn emit_cast_int_to_int_carries_narrow_target() {
    let mut body = Body::dummy(DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(0),
    ));
    let dest = LocalIdx::from_usize(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    body.basic_blocks[BasicBlockIdx::from_usize(0)]
        .statements
        .push(Statement {
            kind: StatementKind::Assign(
                Place::new(dest),
                Rvalue::Cast(
                    CastKind::IntToInt,
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(-1),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    Ty::U8,
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    // MIR-14: `(-1) as u8` must be able to produce 255 in the VM, so the
    // emitter has to name the narrow target (`CastTarget::U8`) — the VM's
    // truncation needs the width, which a bare kind byte cannot carry.
    assert_eq!(
        &result[9..12],
        &[OP_CAST, 0, glyim_bytecode_vm::CastTarget::U8 as u8],
        "IntToInt over a u8 target: kind 0, target U8"
    );
}
