use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{TyCtxMut, TyKind};

fn test_binop_i32(op: BinOp, lhs_val: i64, rhs_val: i64, expected_store_val: i32) {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();

    let lhs = const_operand_i32(lhs_val, i32_ty);
    let rhs = const_operand_i32(rhs_val, i32_ty);
    let rv = Rvalue::BinaryOp(op, box_operands(lhs, rhs));

    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains(&format!("store i32 {expected_store_val}")),
        "Expected 'store i32 {expected_store_val}' for op {op:?}, got IR:\n{ir}"
    );
}

fn test_binop_u32(op: BinOp, lhs_val: u64, rhs_val: u64, expected_store_val: u32) {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let u32_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U32));
    let frozen = ctx_mut.freeze();

    let lhs = const_operand_u32(lhs_val, u32_ty);
    let rhs = const_operand_u32(rhs_val, u32_ty);
    let rv = Rvalue::BinaryOp(op, box_operands(lhs, rhs));

    let body = simple_mir_body(u32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains(&format!("store i32 {expected_store_val}")),
        "Expected 'store i32 {expected_store_val}' for op {op:?}, got IR:\n{ir}"
    );
}

#[test]
fn test_add_i32() {
    test_binop_i32(BinOp::Add, 10, 32, 42);
}

#[test]
fn test_sub_i32() {
    test_binop_i32(BinOp::Sub, 50, 18, 32);
}

#[test]
fn test_mul_i32() {
    test_binop_i32(BinOp::Mul, 7, 6, 42);
}

#[test]
fn test_sdiv_i32() {
    test_binop_i32(BinOp::Div, 84, 2, 42);
}

#[test]
fn test_srem_i32() {
    test_binop_i32(BinOp::Rem, 85, 43, 42);
}

#[test]
fn test_udiv_u32() {
    test_binop_u32(BinOp::Div, 86, 2, 43);
}

#[test]
fn test_urem_u32() {
    test_binop_u32(BinOp::Rem, 87, 3, 0);
}

#[test]
fn test_add_u32() {
    test_binop_u32(BinOp::Add, 20, 22, 42);
}

#[test]
fn test_sub_u32() {
    test_binop_u32(BinOp::Sub, 100, 58, 42);
}

#[test]
fn test_mul_u32() {
    test_binop_u32(BinOp::Mul, 6, 7, 42);
}
