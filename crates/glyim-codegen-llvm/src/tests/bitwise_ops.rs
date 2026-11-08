use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{TyCtxMut, TyKind};

#[test]
fn test_bitand_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(0b1100, i32_ty);
    let rhs = const_operand_i32(0b1010, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::BitAnd, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("store i32 8"), "Expected 'store i32 8':\n{ir}");
}

#[test]
fn test_bitor_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(0b1100, i32_ty);
    let rhs = const_operand_i32(0b1010, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::BitOr, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i32 14"),
        "Expected 'store i32 14':\n{ir}"
    );
}

#[test]
fn test_bitxor_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(0b1100, i32_ty);
    let rhs = const_operand_i32(0b1010, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::BitXor, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("store i32 6"), "Expected 'store i32 6':\n{ir}");
}

#[test]
fn test_shl_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(1, i32_ty);
    let rhs = const_operand_i32(3, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Shl, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("store i32 8"), "Expected 'store i32 8':\n{ir}");
}

#[test]
fn test_shr_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(16, i32_ty);
    let rhs = const_operand_i32(2, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Shr, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("store i32 4"), "Expected 'store i32 4':\n{ir}");
}
