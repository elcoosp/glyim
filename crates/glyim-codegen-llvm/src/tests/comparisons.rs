use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{Ty, TyCtxMut, TyKind};

#[test]
fn test_eq_i32_true() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(5, i32_ty);
    let rhs = const_operand_i32(5, i32_ty);
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
        "Expected 'store i1 true':\n{}",
        ir
    );
}

#[test]
fn test_eq_i32_false() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(5, i32_ty);
    let rhs = const_operand_i32(7, i32_ty);
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
        "Expected 'store i1 false':\n{}",
        ir
    );
}

#[test]
fn test_ne_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(5, i32_ty);
    let rhs = const_operand_i32(7, i32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Ne, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{}",
        ir
    );
}

#[test]
fn test_lt_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(5, i32_ty);
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
        "Expected 'store i1 true':\n{}",
        ir
    );
}

#[test]
fn test_gt_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(10, i32_ty);
    let rhs = const_operand_i32(5, i32_ty);
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
        "Expected 'store i1 true':\n{}",
        ir
    );
}

#[test]
fn test_le_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(5, i32_ty);
    let rhs = const_operand_i32(5, i32_ty);
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
        "Expected 'store i1 true':\n{}",
        ir
    );
}

#[test]
fn test_ge_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_i32(10, i32_ty);
    let rhs = const_operand_i32(5, i32_ty);
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
        "Expected 'store i1 true':\n{}",
        ir
    );
}

/// LL-1 regression: a `u32`/`u64`/`usize` comparison must use the *unsigned*
/// predicate. A value >= 2^63 compared as signed gives the wrong answer.
#[test]
fn test_gt_u32_uses_unsigned_predicate() {
    use glyim_core::primitives::UintTy;
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_u32(3_000_000_000u32, u32_ty);
    let rhs = const_operand_u32(4u32, u32_ty);
    let rv = Rvalue::BinaryOp(BinOp::Gt, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // Constant operands are folded at IR-build time, so assert on the folded
    // result rather than the instruction. With a *signed* predicate,
    // `3_000_000_000u32` reinterpreted as i32 is negative, so `> 4` would fold
    // to `false`; the unsigned predicate folds to `true` (LL-1).
    assert!(
        ir.contains("store i1 true"),
        "`3_000_000_000u32 > 4` must fold to true (unsigned); got:\n{ir}"
    );
}
