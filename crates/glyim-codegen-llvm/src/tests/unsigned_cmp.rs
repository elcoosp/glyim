use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{Ty, TyCtxMut, TyKind};

#[test]
fn test_ult_u32_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(5u64, u32_ty);
    let rhs = const_operand_u32(10u64, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Lt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}

#[test]
fn test_ult_u32_false() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(10u64, u32_ty);
    let rhs = const_operand_u32(5u64, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Lt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 false"),
        "Expected 'store i1 false':\n{ir}"
    );
}

#[test]
fn test_ugt_u32_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(10u64, u32_ty);
    let rhs = const_operand_u32(5u64, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Gt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}

#[test]
fn test_ule_u32_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(5u64, u32_ty);
    let rhs = const_operand_u32(5u64, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::LtEq, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}

#[test]
fn test_uge_u32_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(10u64, u32_ty);
    let rhs = const_operand_u32(5u64, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::GtEq, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}

#[test]
fn test_slt_negative_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(-5, i32_ty);
    let rhs = const_operand_i32(10, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Lt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}

#[test]
fn test_slt_negative_i32_false() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(10, i32_ty);
    let rhs = const_operand_i32(-5, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Lt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 false"),
        "Expected 'store i1 false':\n{ir}"
    );
}
