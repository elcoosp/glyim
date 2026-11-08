//! ABI classification tests: verify `PassMode` decisions for different types

use glyim_core::{Abi, FloatTy, IntTy, Interner, Safety, TargetInfo, UintTy};
use glyim_layout::LayoutComputer;
use glyim_type::{FnSig, GenericArg, TyCtxMut, TyKind};

use crate::abi::FullLayoutComputer;

#[test]
fn scalar_types_pass_direct() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let target_info = TargetInfo::default();

    for kind in [
        TyKind::Bool,
        TyKind::Int(IntTy::I32),
        TyKind::Uint(UintTy::U64),
        TyKind::Float(FloatTy::F64),
    ] {
        let ty = ctx.mk_ty(kind.clone());
        let empty_inputs = ctx.intern_substitution(vec![]);
        let fn_sig = FnSig {
            inputs: empty_inputs,
            output: ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let frozen = ctx.freeze();
        let layout_computer = FullLayoutComputer::new(&frozen, target_info.clone());
        let fn_abi = layout_computer
            .fn_abi_of(&fn_sig)
            .expect("fn_abi_of should succeed");
        assert!(
            matches!(fn_abi.ret.mode, glyim_layout::PassMode::Direct),
            "{:?} should pass directly, got {:?}",
            kind,
            fn_abi.ret.mode
        );
        ctx = TyCtxMut::new(Interner::default());
    }
}

#[test]
fn small_tuple_pass_direct() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let small_tuple_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(small_tuple_subst));
    let empty_inputs = ctx.intern_substitution(vec![]);
    let fn_sig = FnSig {
        inputs: empty_inputs,
        output: tuple_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert!(
        matches!(fn_abi.ret.mode, glyim_layout::PassMode::Direct),
        "Small tuple (8 bytes) should pass directly, got {:?}",
        fn_abi.ret.mode
    );
}

#[test]
fn large_tuple_pass_indirect() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    let large_tuple_subst = ctx.intern_substitution(vec![
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
        GenericArg::Ty(i64_ty),
    ]);
    let large_tuple = ctx.mk_ty(TyKind::Tuple(large_tuple_subst));
    let empty_inputs = ctx.intern_substitution(vec![]);
    let fn_sig = FnSig {
        inputs: empty_inputs,
        output: large_tuple,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert!(
        matches!(fn_abi.ret.mode, glyim_layout::PassMode::Indirect { .. }),
        "Large tuple (40 bytes) should pass indirectly, got {:?}",
        fn_abi.ret.mode
    );
}

#[test]
fn pointer_sized_types_pass_direct() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i64_ty = ctx.mk_ty(TyKind::Int(IntTy::I64));
    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(i64_ty)]),
        output: ctx.mk_ty(TyKind::Unit),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert!(
        matches!(fn_abi.args[0].mode, glyim_layout::PassMode::Direct),
        "i64 arg should pass directly, got {:?}",
        fn_abi.args[0].mode
    );
}

#[test]
fn c_abi_call_convention() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let fn_sig = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::C,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert_eq!(fn_abi.conv, glyim_layout::CallConvention::C);
}

#[test]
fn glyim_abi_call_convention() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let fn_sig = FnSig {
        inputs,
        output: i32_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert_eq!(fn_abi.conv, glyim_layout::CallConvention::Glyim);
}

#[test]
fn unit_return_ignored() {
    let mut ctx = TyCtxMut::new(Interner::default());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let unit_ty = ctx.mk_ty(TyKind::Unit);
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let fn_sig = FnSig {
        inputs,
        output: unit_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let target_info = TargetInfo::default();
    let frozen = ctx.freeze();
    let layout_computer = FullLayoutComputer::new(&frozen, target_info);
    let fn_abi = layout_computer
        .fn_abi_of(&fn_sig)
        .expect("fn_abi_of should succeed");
    assert!(
        matches!(fn_abi.ret.mode, glyim_layout::PassMode::Ignore),
        "Unit return should be Ignore, got {:?}",
        fn_abi.ret.mode
    );
}
