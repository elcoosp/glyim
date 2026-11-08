use super::helpers::*;
use glyim_core::primitives::*;
use glyim_mir::Rvalue;
use glyim_type::{Ty, TyCtxMut};

#[test]
fn test_and_bool() {
    let ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_bool(true);
    let rhs = const_operand_bool(false);
    let rv = Rvalue::BinaryOp(BinOp::And, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // LLVM constant-folds true & false = false
    assert!(
        ir.contains("store i1 false"),
        "Expected 'store i1 false':\n{ir}"
    );
}

#[test]
fn test_or_bool() {
    let ctx_mut = TyCtxMut::new(glyim_core::Interner::default());
    let frozen = ctx_mut.freeze();
    let lhs = const_operand_bool(true);
    let rhs = const_operand_bool(false);
    let rv = Rvalue::BinaryOp(BinOp::Or, box_operands(lhs, rhs));
    let body = simple_mir_body(Ty::BOOL, rv);
    let backend = crate::LlvmBackend::new().with_ty_ctx(frozen);
    let context = inkwell::context::Context::create();
    let module = backend
        .lower_body_to_module(&context, &body)
        .expect("lowering");
    let ir = module.print_to_string().to_string();
    // LLVM constant-folds true | false = true
    assert!(
        ir.contains("store i1 true"),
        "Expected 'store i1 true':\n{ir}"
    );
}
