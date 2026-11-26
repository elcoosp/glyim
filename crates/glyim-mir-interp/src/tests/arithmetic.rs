use super::common::*;
use crate::*;
use glyim_core::IntTy;
use glyim_mir::LocalIdx;
use glyim_test::test_ty_ctx;
use glyim_type::TyKind;

#[test]
fn interpret_integer_add() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_add_body(&tcx_mut, 3, 4, i32_ty);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    let val = interp.get_local_value(LocalIdx::from_usize(1)).unwrap();
    assert_eq!(val, &InterpValue::Int(7));
}

#[test]
fn interpret_integer_sub() {
    let mut tcx_mut = test_ty_ctx();
    let i32_ty = tcx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_sub_body(&tcx_mut, 10, 3, i32_ty);
    let tcx = tcx_mut.freeze();
    let mut interp = Interpreter::new(&tcx);
    interp.run_body(&body).unwrap();
    let val = interp.get_local_value(LocalIdx::from_usize(1)).unwrap();
    assert_eq!(val, &InterpValue::Int(7));
}
