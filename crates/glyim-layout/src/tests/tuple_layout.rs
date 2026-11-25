//! Tuple and array layout computation tests

use crate::*;
use glyim_core::primitives::*;
use glyim_test::with_fresh_ty_ctx;

#[test]
fn s15_tuple_unit() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_tuple(substs)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(tuple_ty)
        .expect("unit tuple should succeed");
    assert_eq!(layout.size, Size::ZERO);
    assert_eq!(layout.align, Align::ONE);
}

#[test]
fn s15_tuple_single_i32() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        c.mk_tuple(substs)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(tuple_ty)
        .expect("tuple layout should succeed");
    assert_eq!(layout.size.as_u64(), 4);
    assert_eq!(layout.align.as_u64(), 4);
}

#[test]
fn s15_tuple_two_i32() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        c.mk_tuple(substs)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(tuple_ty)
        .expect("tuple layout should succeed");
    assert_eq!(layout.size.as_u64(), 8);
    assert_eq!(layout.align.as_u64(), 4);
}

#[test]
fn s15_tuple_bool_and_i32() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![
            glyim_type::GenericArg::Ty(bool_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        c.mk_tuple(substs)
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(tuple_ty)
        .expect("tuple layout should succeed");
    assert_eq!(
        layout.size.as_u64(),
        8,
        "tuple (bool, i32) size = 8 with padding"
    );
    assert_eq!(layout.align.as_u64(), 4);
}

#[test]
fn s15_tuple_array_layout() {
    let (ctx, arr_ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let four = glyim_type::Const {
            kind: glyim_type::ConstKind::Uint(4),
            ty: c.mk_ty(glyim_type::TyKind::Uint(UintTy::Usize)),
        };
        c.mk_ty(glyim_type::TyKind::Array(i32_ty, four))
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let layout = computer
        .layout_of(arr_ty)
        .expect("array layout should succeed");
    assert_eq!(layout.size.as_u64(), 16, "[i32; 4] size = 16");
    assert_eq!(layout.align.as_u64(), 4, "[i32; 4] align = 4");
    match &layout.fields {
        FieldsShape::Array { stride, count } => {
            assert_eq!(*stride, Size::bytes(4));
            assert_eq!(*count, 4);
        }
        _ => panic!("expected Array fields shape"),
    }
}
