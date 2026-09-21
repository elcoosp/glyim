//! Tests for field_ty lookup (S04-T02)

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;

use super::helpers::with_fresh_ty_ctx;
use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::ty::{FieldIdx, TyKind};

/// S04-T02: `field_ty` returns correct type for ADT field index via adt_defs.
#[test]
fn field_ty_returns_correct_type_from_adt_def() {
    let (frozen, (adt_id, i32_ty, bool_ty)) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = ctx.bool_ty();

        let name_a = ctx.resolver().intern("a");
        let name_b = ctx.resolver().intern("b");
        let adt_id = AdtId::from_raw(200);
        let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
        fields.push(FieldDef {
            name: name_a,
            ty: i32_ty,
        });
        fields.push(FieldDef {
            name: name_b,
            ty: bool_ty,
        });

        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields,
            variants: vec![VariantDef {
                name: name_a,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };

        ctx.register_adt(adt_id, adt_def);
        let substs = ctx.intern_substitution(vec![]);
        let _adt_ty = ctx.mk_adt(adt_id, substs);
        (adt_id, i32_ty, bool_ty)
    });

    // Check field_ty on frozen TyCtx
    assert_eq!(frozen.field_ty(adt_id, 0), i32_ty, "field 0 should be i32");
    assert_eq!(
        frozen.field_ty(adt_id, 1),
        bool_ty,
        "field 1 should be bool"
    );
    assert_eq!(
        frozen.field_ty(adt_id, 2),
        frozen.error_ty(),
        "field 2 should be error (out of bounds)"
    );
}

/// S04-T02b: `field_ty` on TyCtxMut returns correct types.
#[test]
fn field_ty_mut_returns_correct_type_from_adt_def() {
    use super::helpers::test_ty_ctx;

    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();

    let name_a = ctx.resolver().intern("a");
    let name_b = ctx.resolver().intern("b");
    let adt_id = AdtId::from_raw(200);
    let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
    fields.push(FieldDef {
        name: name_a,
        ty: i32_ty,
    });
    fields.push(FieldDef {
        name: name_b,
        ty: bool_ty,
    });

    let adt_def = AdtDef {
        kind: AdtKind::Struct,
        fields,
        variants: vec![VariantDef {
            name: name_a,
            style: crate::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }],
        generic_params: vec![],
    };

    ctx.register_adt(adt_id, adt_def);
    let substs = ctx.intern_substitution(vec![]);
    let _adt_ty = ctx.mk_adt(adt_id, substs);

    assert_eq!(
        ctx.field_ty(adt_id, 0),
        i32_ty,
        "mut: field 0 should be i32"
    );
    assert_eq!(
        ctx.field_ty(adt_id, 1),
        bool_ty,
        "mut: field 1 should be bool"
    );
    assert_eq!(
        ctx.field_ty(adt_id, 2),
        ctx.error_ty(),
        "mut: field 2 should be error"
    );
}

/// S04-T02c: `field_ty` falls back to adt_reprs when adt_defs has no entry.
#[test]
fn field_ty_falls_back_to_adt_reprs() {
    let (frozen, (adt_id, i32_ty, bool_ty)) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = ctx.bool_ty();

        let adt_id = AdtId::from_raw(201);
        // Only register adt_reprs, NOT adt_defs
        ctx.register_adt_repr(adt_id, vec![i32_ty, bool_ty]);
        (adt_id, i32_ty, bool_ty)
    });

    assert_eq!(
        frozen.field_ty(adt_id, 0),
        i32_ty,
        "repr fallback: field 0 should be i32"
    );
    assert_eq!(
        frozen.field_ty(adt_id, 1),
        bool_ty,
        "repr fallback: field 1 should be bool"
    );
}

/// S04-T02d: `field_ty` returns error_ty for unknown ADT.
#[test]
fn field_ty_returns_error_for_unknown_adt() {
    let (frozen, adt_id) = with_fresh_ty_ctx(|_| AdtId::from_raw(999));
    assert_eq!(
        frozen.field_ty(adt_id, 0),
        frozen.error_ty(),
        "unknown ADT should yield error_ty"
    );
}
