use crate::*;
use glyim_core::AdtId;
use glyim_core::primitives::*;
use glyim_test::{assert_layout, test_frozen_ty_ctx, with_fresh_ty_ctx};
use glyim_type::{FnSig, GenericArg, Region};

// ========== Unsigned integer layouts ==========
#[test]
fn s04_e01_layout_u8() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U8)));
    assert_layout(&ctx, ty, 1, 1);
}

#[test]
fn s04_e02_layout_u16() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U16)));
    assert_layout(&ctx, ty, 2, 2);
}

#[test]
fn s04_e03_layout_u32() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U32)));
    assert_layout(&ctx, ty, 4, 4);
}

#[test]
fn s04_e04_layout_u64() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::U64)));
    assert_layout(&ctx, ty, 8, 8);
}

// ========== Pointer-sized integers ==========
#[test]
fn s04_e05_layout_isize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::Isize)));
    assert_layout(&ctx, ty, 8, 8); // x86_64
}

#[test]
fn s04_e06_layout_usize() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Uint(UintTy::Usize)));
    assert_layout(&ctx, ty, 8, 8); // x86_64
}

// ========== Char ==========
#[test]
fn s04_e07_layout_char() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Char));
    assert_layout(&ctx, ty, 4, 4);
}

// ========== Error type returns UnknownType ==========
#[test]
fn s04_e08_error_type_is_unknown() {
    let ctx = test_frozen_ty_ctx();
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let result = computer.layout_of(Ty::ERROR);
    assert!(matches!(result, Err(LayoutError::UnknownType(t)) if t == Ty::ERROR));
}

// ========== Unknown ADT type returns UnknownType ==========
#[test]
fn s04_e09_unknown_adt_type() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Adt(AdtId::from_usize(42), substs))
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let result = computer.layout_of(adt_ty);
    assert!(matches!(result, Err(LayoutError::UnknownType(_))));
}

// ========== fn_abi_of: C calling convention ==========
#[test]
fn s04_e10_fn_abi_of_c_conv() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::C,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).unwrap();
    assert_eq!(abi.conv, CallConvention::C);
    assert_eq!(abi.args.len(), 1);
}

// ========== fn_abi_of: variadic ==========
#[test]
fn s04_e11_fn_abi_of_variadic() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        let inputs = c.intern_substitution(vec![GenericArg::Ty(bool_ty)]);
        FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: true,
            unsafety: Safety::Unsafe,
            abi: Abi::C,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).unwrap();
    assert!(abi.c_variadic);
    assert_eq!(abi.args.len(), 1);
}

// ========== fn_abi_of: filters non-type args ==========
#[test]
fn s04_e12_fn_abi_of_ignores_lifetime_args() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let unit_ty = c.unit_ty();
        // Lifetime arg (GenericArg::Lifetime) and a type arg
        let inputs = c.intern_substitution(vec![
            GenericArg::Lifetime(Region::Erased),
            GenericArg::Ty(unit_ty),
        ]);
        FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).unwrap();
    assert_eq!(abi.args.len(), 1);
}

// ========== Size & Align utilities ==========
#[test]
fn s04_e13_size_arithmetic() {
    let s = Size::bytes(5);
    assert_eq!(s.checked_bits(), Some(40));
    let aligned = s.checked_align_to(Align::from_bytes(4).expect("4 is a power of two"));
    assert_eq!(aligned, Some(Size::bytes(8)));
    let sum = Size::bytes(3).checked_add(Size::bytes(7));
    assert_eq!(sum, Some(Size::bytes(10)));
}

#[test]
fn s04_e14_align_max() {
    let a = Align::from_bytes(2);
    let b = Align::from_bytes(8);
    assert_eq!(a.max(b), b);
}

// ========== LayoutComputer methods ==========
#[test]
fn s04_e15_ptr_size_and_align() {
    let ctx = test_frozen_ty_ctx();
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    assert_eq!(computer.ptr_size(), Size::bytes(8));
    assert_eq!(computer.ptr_align().as_u64(), 8);
}

#[test]
fn s04_e16_target_info() {
    let ctx = test_frozen_ty_ctx();
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    assert_eq!(computer.target_info().pointer_size(), 8);
}

// ========== CallConvention mapping ==========
#[test]
fn s04_e17_call_conv_from_abi() {
    assert_eq!(CallConvention::from(Abi::Glyim), CallConvention::Glyim);
    assert_eq!(CallConvention::from(Abi::C), CallConvention::C);
    assert_eq!(CallConvention::from(Abi::System), CallConvention::System);
}
