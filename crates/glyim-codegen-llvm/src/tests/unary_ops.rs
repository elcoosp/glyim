use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{Ty, TyCtxMut, TyKind};

#[test]
fn test_not_bool() {
    let ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let frozen = ctx_mut.freeze();
    let op = const_operand_bool(true);
    let rv = Rvalue::UnaryOp(UnOp::Not, op);
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
fn test_not_bool_false() {
    let ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let frozen = ctx_mut.freeze();
    let op = const_operand_bool(false);
    let rv = Rvalue::UnaryOp(UnOp::Not, op);
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
fn test_neg_i32() {
    let mut ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let frozen = ctx_mut.freeze();
    let op = const_operand_i32(42, i32_ty);
    let rv = Rvalue::UnaryOp(UnOp::Neg, op);
    let body = simple_mir_body(i32_ty, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    assert!(
        ir.contains("store i32 -42"),
        "Expected 'store i32 -42':\n{ir}"
    );
}
