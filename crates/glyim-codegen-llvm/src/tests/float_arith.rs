use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{Ty, TyCtxMut, TyKind};

#[test]
fn test_fadd_f64() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(2.5, f64_ty);
    let rhs = const_operand_f64(3.5, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Add, box_operands(lhs, rhs));
    let body = simple_mir_body(f64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store double 6"),
        "Expected 'store double 6':\n{ir}"
    );
}

#[test]
fn test_fsub_f64() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(10.0, f64_ty);
    let rhs = const_operand_f64(3.0, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Sub, box_operands(lhs, rhs));
    let body = simple_mir_body(f64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store double 7"),
        "Expected 'store double 7':\n{ir}"
    );
}

#[test]
fn test_fmul_f64() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(6.0, f64_ty);
    let rhs = const_operand_f64(7.0, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Mul, box_operands(lhs, rhs));
    let body = simple_mir_body(f64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store double 4.2"),
        "Expected 'store double 4.2e+01' or similar:\n{ir}"
    );
}

#[test]
fn test_fdiv_f64() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(10.0, f64_ty);
    let rhs = const_operand_f64(2.0, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Div, box_operands(lhs, rhs));
    let body = simple_mir_body(f64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store double 5"),
        "Expected 'store double 5':\n{ir}"
    );
}

#[test]
fn test_feq_f64_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(std::f64::consts::PI, f64_ty);
    let rhs = const_operand_f64(std::f64::consts::PI, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Eq, box_operands(lhs, rhs));
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
fn test_feq_f64_false() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(1.0, f64_ty);
    let rhs = const_operand_f64(2.0, f64_ty);
    let rv = Rvalue::BinaryOp(BinOp::Eq, box_operands(lhs, rhs));
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
fn test_flt_f64() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_f64(1.0, f64_ty);
    let rhs = const_operand_f64(2.0, f64_ty);
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
fn test_fneg_unary() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let f64_ty = ctx_mut.mk_ty(TyKind::Float(FloatTy::F64));
    let frozen = ctx_mut.freeze();
    let op = const_operand_f64(42.0, f64_ty);
    let rv = Rvalue::UnaryOp(UnOp::Neg, op);
    let body = simple_mir_body(f64_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // LLVM constant-folds fneg 42.0 -> -42.0
    assert!(
        ir.contains("store double -4.200000e+01") || ir.contains("fneg"),
        "Expected 'store double -4.200000e+01' or 'fneg' in IR:\n{ir}"
    );
}
