use super::super::*;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_core::primitives::{BinOp, Mutability};
use glyim_span::Span;
use glyim_type::Ty;
use std::sync::Arc;

fn make_body(op: BinOp) -> Body {
    let mut body = Body::dummy(DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0)));
    let dest_local = LocalIdx::from_raw(1);
    body.locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(dest_local),
            Rvalue::BinaryOp(
                op,
                [
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(3),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                    Operand::Constant(MirConst {
                        kind: MirConstKind::Int(5),
                        ty: Ty::ERROR,
                        span: Span::DUMMY,
                    }),
                ],
            ),
        ),
        source_info: SourceInfo::new(Span::DUMMY),
    };
    body.basic_blocks[BasicBlockIdx::from_raw(0)]
        .statements
        .push(stmt);
    body
}

#[test]
fn emit_add() {
    let body = make_body(BinOp::Add);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    // OP_LOAD_CONST + 3i64, OP_LOAD_CONST + 5i64, OP_ADD, OP_STORE_LOCAL + 1, OP_TRAP
    // OP_LOAD_CONST(0x01) + 3i64, OP_LOAD_CONST + 5i64, OP_ADD(0x02), OP_STORE_LOCAL(0x17) + 1u32
    let mut expected = Vec::new();
    expected.push(0x01); // OP_LOAD_CONST
    expected.extend_from_slice(&3i64.to_le_bytes());
    expected.push(0x01); // OP_LOAD_CONST
    expected.extend_from_slice(&5i64.to_le_bytes());
    expected.push(0x02); // OP_ADD
    expected.push(0x17); // OP_STORE_LOCAL
    expected.extend_from_slice(&1u32.to_le_bytes());
    // Note: dummy body has Unreachable terminator, which now emits OP_TRAP
    expected.push(OP_TRAP);
    assert_eq!(result, expected);
}

#[test]
fn emit_sub() {
    let body = make_body(BinOp::Sub);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    // Should contain OP_SUB instead of OP_ADD
    assert!(result.contains(&OP_SUB));
}

#[test]
fn emit_mul() {
    let body = make_body(BinOp::Mul);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_MUL));
}

#[test]
fn emit_div() {
    let body = make_body(BinOp::Div);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_DIV));
}

#[test]
fn emit_rem() {
    let body = make_body(BinOp::Rem);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_REM));
}

#[test]
fn emit_eq() {
    let body = make_body(BinOp::Eq);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_EQ));
}

#[test]
fn emit_ne() {
    let body = make_body(BinOp::Ne);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_NE));
}

#[test]
fn emit_lt() {
    let body = make_body(BinOp::Lt);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_LT));
}

#[test]
fn emit_gt() {
    let body = make_body(BinOp::Gt);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_GT));
}

#[test]
fn emit_le() {
    let body = make_body(BinOp::LtEq);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_LE));
}

#[test]
fn emit_ge() {
    let body = make_body(BinOp::GtEq);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_GE));
}

#[test]
fn emit_and() {
    let body = make_body(BinOp::And);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_AND));
}

#[test]
fn emit_or() {
    let body = make_body(BinOp::Or);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_OR));
}

#[test]
fn emit_bitand() {
    let body = make_body(BinOp::BitAnd);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_BITAND));
}

#[test]
fn emit_bitor() {
    let body = make_body(BinOp::BitOr);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_BITOR));
}

#[test]
fn emit_bitxor() {
    let body = make_body(BinOp::BitXor);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_BITXOR));
}

#[test]
fn emit_shl() {
    let body = make_body(BinOp::Shl);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_SHL));
}

#[test]
fn emit_shr() {
    let body = make_body(BinOp::Shr);
    let backend = BytecodeBackend::with_ty_ctx(
        std::sync::Arc::new(glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze()),
        glyim_core::TargetInfo::default(),
    );
    let result = backend.generate_function(&Arc::new(body)).unwrap();
    assert!(result.contains(&OP_SHR));
}
