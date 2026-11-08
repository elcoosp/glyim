//! S15-T03: `fn_abi_of` respects `SystemV` and AAPCS calling conventions

use crate::*;
use glyim_core::primitives::*;
use glyim_test::with_fresh_ty_ctx;

#[test]
fn s15_t03_fn_abi_systemv_i32_ret_i32() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        glyim_type::FnSig {
            inputs,
            output: i32_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::C,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(abi.conv, CallConvention::C);
    assert_eq!(abi.args.len(), 1);
    assert_eq!(abi.args[0].mode, PassMode::Direct);
    assert_eq!(abi.ret.mode, PassMode::Direct);
}

#[test]
fn s15_t03_fn_abi_aapcs_on_aarch64() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i64_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I64));
        let f64_ty = c.mk_ty(glyim_type::TyKind::Float(FloatTy::F64));
        let inputs = c.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i64_ty),
            glyim_type::GenericArg::Ty(f64_ty),
        ]);
        glyim_type::FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::C,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::aarch64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(abi.conv, CallConvention::C);
    assert_eq!(abi.args.len(), 2);
    assert_eq!(abi.args[0].mode, PassMode::Direct);
    assert_eq!(abi.args[1].mode, PassMode::Direct);
}

#[test]
fn s15_t03_fn_abi_unit_ret_ignored() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        glyim_type::FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(
        abi.ret.mode,
        PassMode::Ignore,
        "unit return should be Ignore"
    );
}

#[test]
fn s15_t03_fn_abi_glyim_conv() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let bool_ty = c.bool_ty();
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(bool_ty)]);
        glyim_type::FnSig {
            inputs,
            output: bool_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(abi.conv, CallConvention::Glyim);
}

#[test]
fn s15_t03_fn_abi_system_conv() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        glyim_type::FnSig {
            inputs,
            output: i32_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::System,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(abi.conv, CallConvention::System);
}

#[test]
fn s15_t03_fn_abi_c_variadic_flag() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        glyim_type::FnSig {
            inputs,
            output: c.unit_ty(),
            c_variadic: true,
            unsafety: Safety::Safe,
            abi: Abi::C,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert!(abi.c_variadic);
}

#[test]
fn s15_t03_fn_abi_never_ret_ignore() {
    let (ctx, fn_sig) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(glyim_type::TyKind::Int(IntTy::I32));
        let inputs = c.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
        glyim_type::FnSig {
            inputs,
            output: c.never_ty(),
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        }
    });
    let computer = SimpleLayoutComputer::new(&ctx, TargetInfo::x86_64());
    let abi = computer.fn_abi_of(&fn_sig).expect("fn_abi should succeed");
    assert_eq!(
        abi.ret.mode,
        PassMode::Ignore,
        "never return should be Ignore"
    );
}
