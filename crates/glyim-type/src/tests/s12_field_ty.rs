//! S12-T02: TypeLookup::field_ty resolves ADT fields via default impl,
//! and TyCtxMut/TyCtx field_ty methods work correctly.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::interner::Interner;
use glyim_core::primitives::IntTy;

use super::helpers::{test_frozen_ty_ctx, test_ty_ctx, with_fresh_ty_ctx};
use crate::display::TypeLookup;
use crate::*;

// ---- field_ty on TyCtxMut ----

#[test]
fn field_ty_from_adt_def() {
    let mut ctx = test_ty_ctx();
    let adt_id = AdtId::from_raw(100);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let mut fields = IndexVec::new();
    fields.push(FieldDef {
        name: Interner::default().intern("x"),
        ty: i32_ty,
    });
    fields.push(FieldDef {
        name: Interner::default().intern("y"),
        ty: bool_ty,
    });
    let def = AdtDef {
        kind: AdtKind::Struct,
        fields,
        variants: vec![],
        generic_params: vec![],
    };
    ctx.register_adt(adt_id, def);
    assert_eq!(ctx.field_ty(adt_id, 0), i32_ty);
    assert_eq!(ctx.field_ty(adt_id, 1), bool_ty);
}

#[test]
fn field_ty_from_adt_repr_fallback() {
    let mut ctx = test_ty_ctx();
    let adt_id = AdtId::from_raw(101);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    ctx.register_adt_repr(adt_id, vec![i32_ty]);
    assert_eq!(ctx.field_ty(adt_id, 0), i32_ty);
}

#[test]
fn field_ty_out_of_bounds_returns_error() {
    let ctx = test_ty_ctx();
    let adt_id = AdtId::from_raw(102);
    assert_eq!(ctx.field_ty(adt_id, 0), Ty::ERROR);
}

#[test]
fn field_ty_unknown_adt_returns_error() {
    let ctx = test_ty_ctx();
    let adt_id = AdtId::from_raw(103);
    assert_eq!(ctx.field_ty(adt_id, 5), Ty::ERROR);
}

#[test]
fn field_ty_adt_def_takes_priority_over_repr() {
    let mut ctx = test_ty_ctx();
    let adt_id = AdtId::from_raw(104);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    ctx.register_adt_repr(adt_id, vec![bool_ty]);
    let mut fields = IndexVec::new();
    fields.push(FieldDef {
        name: Interner::default().intern("z"),
        ty: i32_ty,
    });
    let def = AdtDef {
        kind: AdtKind::Struct,
        fields,
        variants: vec![],
        generic_params: vec![],
    };
    ctx.register_adt(adt_id, def);
    assert_eq!(
        ctx.field_ty(adt_id, 0),
        i32_ty,
        "AdtDef should take priority over AdtRepr"
    );
}

// ---- field_ty on TyCtx (frozen) ----

#[test]
fn frozen_field_ty_from_adt_def() {
    let (frozen, i32_ty) = with_fresh_ty_ctx(|ctx| {
        let adt_id = AdtId::from_raw(200);
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let mut fields = IndexVec::new();
        fields.push(FieldDef {
            name: Interner::default().intern("x"),
            ty: i32_ty,
        });
        let def = AdtDef {
            kind: AdtKind::Struct,
            fields,
            variants: vec![],
            generic_params: vec![],
        };
        ctx.register_adt(adt_id, def);
        i32_ty
    });
    let adt_id = AdtId::from_raw(200);
    assert_eq!(frozen.field_ty(adt_id, 0), i32_ty);
}

#[test]
fn frozen_field_ty_from_adt_repr_fallback() {
    let (frozen, i32_ty) = with_fresh_ty_ctx(|ctx| {
        let adt_id = AdtId::from_raw(201);
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        ctx.register_adt_repr(adt_id, vec![i32_ty]);
        i32_ty
    });
    let adt_id = AdtId::from_raw(201);
    assert_eq!(frozen.field_ty(adt_id, 0), i32_ty);
}

#[test]
fn frozen_field_ty_unknown_adt_returns_error() {
    let frozen = test_frozen_ty_ctx();
    let adt_id = AdtId::from_raw(202);
    assert_eq!(frozen.field_ty(adt_id, 0), Ty::ERROR);
}

// ---- TypeLookup::field_ty default impl ----

struct MinimalTypeLookup {
    error: Ty,
}

impl TypeLookup for MinimalTypeLookup {
    fn ty_kind(&self, ty: Ty) -> &TyKind {
        if ty == self.error {
            static ERROR_KIND: TyKind = TyKind::Error;
            &ERROR_KIND
        } else {
            static BOOL_KIND: TyKind = TyKind::Bool;
            &BOOL_KIND
        }
    }
    fn ty_flags(&self, _ty: Ty) -> TypeFlags {
        TypeFlags::empty()
    }
    fn substitution_args(&self, _sub: Substitution) -> &[GenericArg] {
        &[]
    }
    fn name_str(&self, _name: glyim_core::interner::Name) -> &str {
        ""
    }
    fn error_ty(&self) -> Ty {
        self.error
    }
}

#[test]
fn type_lookup_field_ty_default_returns_error_when_no_adt_def() {
    let error_ty = Ty::ERROR;
    let lookup = MinimalTypeLookup { error: error_ty };
    let adt_id = AdtId::from_raw(999);
    let result = lookup.field_ty(adt_id, 0);
    assert_eq!(
        result, error_ty,
        "default field_ty should return error_ty when adt_def returns None"
    );
}

#[test]
fn type_lookup_adt_def_default_returns_none() {
    let lookup = MinimalTypeLookup { error: Ty::ERROR };
    let adt_id = AdtId::from_raw(999);
    assert!(
        lookup.adt_def(adt_id).is_none(),
        "default adt_def should return None"
    );
}
