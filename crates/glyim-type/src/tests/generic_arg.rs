//! Tests for `GenericArg`, Region, and related types.

use glyim_core::primitives::UintTy;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

// --- GenericArg ---

#[test]
fn generic_arg_ty_variant() {
    let arg = GenericArg::Ty(Ty::BOOL);
    assert!(matches!(arg, GenericArg::Ty(t) if t == Ty::BOOL));
}

#[test]
fn generic_arg_lifetime_variant() {
    let arg = GenericArg::Lifetime(Region::Static);
    assert!(matches!(arg, GenericArg::Lifetime(Region::Static)));
}

#[test]
fn generic_arg_const_variant() {
    let c = Const {
        kind: ConstKind::Uint(42),
        ty: Ty::BOOL,
    };
    let arg = GenericArg::Const(c);
    assert!(matches!(arg, GenericArg::Const(_)));
}

#[test]
fn generic_arg_equality() {
    let a1 = GenericArg::Ty(Ty::BOOL);
    let a2 = GenericArg::Ty(Ty::BOOL);
    let a3 = GenericArg::Ty(Ty::NEVER);
    let a4 = GenericArg::Lifetime(Region::Static);
    assert_eq!(a1, a2);
    assert_ne!(a1, a3);
    assert_ne!(a1, a4);
}

#[test]
fn generic_arg_clone() {
    let a1 = GenericArg::Ty(Ty::BOOL);
    let a2 = a1.clone();
    assert_eq!(a1, a2);
}

#[test]
fn generic_arg_debug() {
    let arg = GenericArg::Ty(Ty::BOOL);
    let debug = format!("{arg:?}");
    assert!(debug.contains("Ty") || debug.contains("BOOL") || debug.contains('3'));
}

// --- Region ---

#[test]
fn region_static() {
    let r = Region::Static;
    assert!(matches!(r, Region::Static));
}

#[test]
fn region_early_bound() {
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("'a");
    let r = Region::EarlyBound(EarlyBoundRegion { index: 0, name });
    assert!(matches!(r, Region::EarlyBound(_)));
}

#[test]
fn region_late_bound() {
    let r = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    assert!(matches!(r, Region::LateBound(_, _, _)));
}

#[test]
fn region_var() {
    let r = Region::Var(RegionVid::from_raw(0));
    assert!(matches!(r, Region::Var(_)));
}

#[test]
fn region_erased() {
    let r = Region::Erased;
    assert!(matches!(r, Region::Erased));
}

#[test]
fn region_error() {
    let r = Region::Error;
    assert!(matches!(r, Region::Error));
}

#[test]
fn region_equality() {
    assert_eq!(Region::Static, Region::Static);
    assert_eq!(Region::Erased, Region::Erased);
    assert_eq!(Region::Error, Region::Error);
    assert_ne!(Region::Static, Region::Erased);
}

#[test]
fn region_clone() {
    let r = Region::Static;
    let r2 = r.clone();
    assert_eq!(r, r2);
}

#[test]
fn region_debug() {
    let r = Region::Static;
    let debug = format!("{r:?}");
    assert!(debug.contains("Static"));
}

// --- EarlyBoundRegion ---

#[test]
fn early_bound_region_fields() {
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("'lifetime");
    let ebr = EarlyBoundRegion { index: 5, name };
    assert_eq!(ebr.index, 5);
}

// --- BoundRegionKind ---

#[test]
fn bound_region_kind_anon() {
    let brk = BoundRegionKind::BrAnon(0);
    assert!(matches!(brk, BoundRegionKind::BrAnon(0)));
}

#[test]
fn bound_region_kind_named() {
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("'a");
    let brk = BoundRegionKind::BrNamed(name);
    assert!(matches!(brk, BoundRegionKind::BrNamed(_)));
}

#[test]
fn bound_region_kind_env() {
    let brk = BoundRegionKind::BrEnv;
    assert!(matches!(brk, BoundRegionKind::BrEnv));
}

// --- Region in substitution ---

#[test]
fn multiple_regions_in_substitution() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        c.intern_substitution(vec![
            GenericArg::Lifetime(Region::Static),
            GenericArg::Lifetime(Region::Erased),
            GenericArg::Lifetime(Region::Error),
        ])
    });
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 3);
    assert!(matches!(&args[0], GenericArg::Lifetime(Region::Static)));
    assert!(matches!(&args[1], GenericArg::Lifetime(Region::Erased)));
    assert!(matches!(&args[2], GenericArg::Lifetime(Region::Error)));
}

#[test]
fn mixed_generic_args_in_substitution() {
    let (ctx, sub) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let cnst = Const {
            kind: ConstKind::Uint(10),
            ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
        };
        c.intern_substitution(vec![
            GenericArg::Lifetime(Region::Erased),
            GenericArg::Ty(i32_ty),
            GenericArg::Const(cnst),
            GenericArg::Ty(c.bool_ty()),
        ])
    });
    let args = ctx.substitution_args(sub);
    assert_eq!(args.len(), 4);
    assert!(matches!(&args[0], GenericArg::Lifetime(_)));
    assert!(matches!(&args[1], GenericArg::Ty(_)));
    assert!(matches!(&args[2], GenericArg::Const(_)));
    assert!(matches!(&args[3], GenericArg::Ty(_)));
}
