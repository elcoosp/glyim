use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{TyCtxMut, TyKind};

#[test]
fn test_i32_max_add_one() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(i64::from(i32::MAX), i32_ty);
    let rhs = const_operand_i32(1, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Add, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // Wrapping: i32::MAX + 1 = i32::MIN = -2147483648
    assert!(
        ir.contains("store i32 -2147483648"),
        "Expected 'store i32 -2147483648':\n{ir}"
    );
}

#[test]
fn test_i32_min_sub_one() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(i64::from(i32::MIN), i32_ty);
    let rhs = const_operand_i32(1, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Sub, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i32 2147483647"),
        "Expected 'store i32 2147483647':\n{ir}"
    );
}

#[test]
fn test_shl_overflow() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(1, i32_ty);
    let rhs = const_operand_i32(31, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Shl, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i32 -2147483648"),
        "Expected 'store i32 -2147483648':\n{ir}"
    );
}

#[test]
fn test_zero_div_sdiv() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(1, i32_ty);
    let rhs = const_operand_i32(0, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Div, box_operands(lhs, rhs));
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // LLVM constant-folds sdiv 1,0 to poison; check for poison or div
    assert!(
        ir.contains("poison") || ir.contains("sdiv") || ir.contains("udiv"),
        "Expected 'poison' or 'sdiv' in IR:\n{ir}"
    );
}

#[test]
fn test_i8_add() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i8_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I8));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(100, i8_ty);
    let rhs = const_operand_i32(27, i8_ty);
    let rv = Rvalue::BinaryOp(BinOp::Add, box_operands(lhs, rhs));
    let body = simple_mir_body(i8_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i8 127"),
        "Expected 'store i8 127':\n{ir}"
    );
}

#[test]
fn test_i64_mul() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i64_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(0x100000000i64, i64_ty); // 4294967296
    let rhs = const_operand_i32(2, i64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Mul, box_operands(lhs, rhs));
    let body = simple_mir_body(i64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i64 8589934592"),
        "Expected 'store i64 8589934592':\n{ir}"
    );
}
