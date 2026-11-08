//! Tests for index types (`TyVar`, `IntVar`, `FloatVar`, `RegionVid`, etc.).

use crate::*;

#[test]
fn ty_var_from_raw_to_raw() {
    let v = TyVar::from_raw(42);
    assert_eq!(v.to_raw(), 42);
}

#[test]
fn int_var_from_raw_to_raw() {
    let v = IntVar::from_raw(100);
    assert_eq!(v.to_raw(), 100);
}

#[test]
fn float_var_from_raw_to_raw() {
    let v = FloatVar::from_raw(200);
    assert_eq!(v.to_raw(), 200);
}

#[test]
fn region_vid_from_raw_to_raw() {
    let v = RegionVid::from_raw(5);
    assert_eq!(v.to_raw(), 5);
}

#[test]
fn const_var_from_raw_to_raw() {
    let v = ConstVar::from_raw(10);
    assert_eq!(v.to_raw(), 10);
}

#[test]
fn field_idx_from_raw_to_raw() {
    let f = FieldIdx::from_raw(3);
    assert_eq!(f.to_raw(), 3);
}

#[test]
fn ty_var_zero() {
    let v = TyVar::from_raw(0);
    assert_eq!(v.to_raw(), 0);
}

#[test]
fn ty_var_large() {
    let v = TyVar::from_raw(u32::MAX);
    assert_eq!(v.to_raw(), u32::MAX);
}

#[test]
fn infer_var_ty_variant() {
    let v = TyVar::from_raw(0);
    let iv = InferVar::Ty(v);
    assert!(matches!(iv, InferVar::Ty(_)));
}

#[test]
fn infer_var_int_variant() {
    let v = IntVar::from_raw(0);
    let iv = InferVar::Int(v);
    assert!(matches!(iv, InferVar::Int(_)));
}

#[test]
fn infer_var_float_variant() {
    let v = FloatVar::from_raw(0);
    let iv = InferVar::Float(v);
    assert!(matches!(iv, InferVar::Float(_)));
}

#[test]
fn infer_var_equality() {
    let iv1 = InferVar::Ty(TyVar::from_raw(5));
    let iv2 = InferVar::Ty(TyVar::from_raw(5));
    let iv3 = InferVar::Ty(TyVar::from_raw(6));
    let iv4 = InferVar::Int(IntVar::from_raw(5));
    assert_eq!(iv1, iv2);
    assert_ne!(iv1, iv3);
    assert_ne!(iv1, iv4);
}

#[test]
fn universe_index_zero() {
    let u = UniverseIndex(0);
    assert_eq!(u.0, 0);
}

#[test]
fn universe_index_large() {
    let u = UniverseIndex(u32::MAX);
    assert_eq!(u.0, u32::MAX);
}

#[test]
fn param_ty_fields() {
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("T");
    let pt = ParamTy { index: 7, name };
    assert_eq!(pt.index, 7);
}

#[test]
fn bound_ty_fields() {
    let bt = BoundTy {
        var: 3,
        kind: BoundTyKind::Anon,
    };
    assert_eq!(bt.var, 3);
    assert!(matches!(bt.kind, BoundTyKind::Anon));
}

#[test]
fn bound_ty_kind_param() {
    let interner = glyim_core::interner::Interner::new();
    let name = interner.intern("T");
    let btk = BoundTyKind::Param(name);
    assert!(matches!(btk, BoundTyKind::Param(_)));
}
