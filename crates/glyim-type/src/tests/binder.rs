//! Tests for Binder, `BoundVariableKind`.

use glyim_core::interner::Interner;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn binder_bind_and_skip() {
    let bound_vars: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Ty(BoundTyKind::Anon)]);
    let binder = Binder::bind(42u32, bound_vars);
    assert_eq!(binder.skip_binder(), 42);
}

#[test]
fn binder_as_ref() {
    let bound_vars: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Ty(BoundTyKind::Anon)]);
    let binder = Binder::bind("hello".to_string(), bound_vars);
    let as_ref = binder.as_ref();
    assert_eq!(*as_ref.value, "hello");
    assert_eq!(as_ref.bound_vars.len(), 1);
}

#[test]
fn binder_with_multiple_bound_vars() {
    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Ty(BoundTyKind::Anon),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Const,
    ]);
    let binder = Binder::bind((), bound_vars);
    assert_eq!(binder.bound_vars.len(), 3);
    assert!(matches!(
        binder.bound_vars[0],
        BoundVariableKind::Ty(BoundTyKind::Anon)
    ));
    assert!(matches!(
        binder.bound_vars[1],
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0))
    ));
    assert!(matches!(binder.bound_vars[2], BoundVariableKind::Const));
}

#[test]
fn binder_equality() {
    let bv1: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Const]);
    let bv2: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Const]);
    let b1 = Binder::bind(10u32, bv1);
    let b2 = Binder::bind(10u32, bv2);
    assert_eq!(b1, b2);
}

#[test]
fn binder_inequality_value() {
    let bv1: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Const]);
    let bv2: Box<[BoundVariableKind]> = Box::new([BoundVariableKind::Const]);
    let b1 = Binder::bind(10u32, bv1);
    let b2 = Binder::bind(20u32, bv2);
    assert_ne!(b1, b2);
}

#[test]
fn bound_variable_kind_ty() {
    let interner = Interner::new();
    let name = interner.intern("T");
    let bv = BoundVariableKind::Ty(BoundTyKind::Param(name));
    assert!(matches!(bv, BoundVariableKind::Ty(_)));
}

#[test]
fn bound_variable_kind_region() {
    let bv = BoundVariableKind::Region(BoundRegionKind::BrEnv);
    assert!(matches!(bv, BoundVariableKind::Region(_)));
}

#[test]
fn bound_ty_kind_anon() {
    let btk = BoundTyKind::Anon;
    assert!(matches!(btk, BoundTyKind::Anon));
}

#[test]
fn bound_ty_kind_param() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let name = c.resolver().intern("T");
        let btk = BoundTyKind::Param(name);
        assert!(matches!(btk, BoundTyKind::Param(_)));
    });
    let _ = ctx;
}

#[test]
fn bound_region_kinds() {
    let br_anon = BoundRegionKind::BrAnon(0);
    assert!(matches!(br_anon, BoundRegionKind::BrAnon(0)));

    let interner = Interner::new();
    let name = interner.intern("'a");
    let br_named = BoundRegionKind::BrNamed(name);
    assert!(matches!(br_named, BoundRegionKind::BrNamed(_)));

    let br_env = BoundRegionKind::BrEnv;
    assert!(matches!(br_env, BoundRegionKind::BrEnv));
}
