//! Tests for interior mutability detection (S04-T03)

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;

use super::helpers::with_fresh_ty_ctx;
use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::display::TypeLookup;
use crate::flags::TypeFlags;
use crate::ty::{FieldIdx, TyKind};

/// S04-T03: `is_interior_mutable_adt` returns true for ADT marked as interior mutable.
#[test]
fn is_interior_mutable_adt_true_when_marked() {
    let (frozen, (adt_id, adt_ty)) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let name_field = ctx.resolver().intern("value");
        let adt_id = AdtId::from_raw(300);
        let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
        fields.push(FieldDef {
            name: name_field,
            ty: i32_ty,
        });

        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields,
            variants: vec![VariantDef {
                name: name_field,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };

        ctx.register_adt(adt_id, adt_def);
        ctx.mark_adt_interior_mutable(adt_id);
        let substs = ctx.intern_substitution(vec![]);
        let adt_ty = ctx.mk_adt(adt_id, substs);
        (adt_id, adt_ty)
    });

    // Check on frozen context via TypeLookup trait
    assert!(
        frozen.is_interior_mutable_adt(adt_id),
        "marked ADT should be interior mutable on TyCtx"
    );
    let frozen_flags = frozen.ty_flags(adt_ty);
    assert!(
        frozen_flags.contains(TypeFlags::HAS_INTERIOR_MUTABILITY),
        "frozen ADT type should have HAS_INTERIOR_MUTABILITY flag"
    );
}

/// S04-T03b: `is_interior_mutable_adt` returns true on TyCtxMut.
#[test]
fn is_interior_mutable_adt_true_on_mut_ctx() {
    use super::helpers::test_ty_ctx;

    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let name_field = ctx.resolver().intern("value");
    let adt_id = AdtId::from_raw(300);
    let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
    fields.push(FieldDef {
        name: name_field,
        ty: i32_ty,
    });

    let adt_def = AdtDef {
        kind: AdtKind::Struct,
        fields,
        variants: vec![VariantDef {
            name: name_field,
            style: crate::adt_def::VariantStyle::Unit,
            fields: IndexVec::new(),
        }],
        generic_params: vec![],
    };

    ctx.register_adt(adt_id, adt_def);
    ctx.mark_adt_interior_mutable(adt_id);
    let substs = ctx.intern_substitution(vec![]);
    let adt_ty = ctx.mk_adt(adt_id, substs);

    assert!(
        ctx.is_interior_mutable_adt(adt_id),
        "marked ADT should be interior mutable on TyCtxMut"
    );
    let flags = ctx.ty_flags(adt_ty);
    assert!(
        flags.contains(TypeFlags::HAS_INTERIOR_MUTABILITY),
        "ADT type should have HAS_INTERIOR_MUTABILITY flag"
    );
}

/// S04-T03c: ADT not marked as interior mutable returns false.
#[test]
fn is_interior_mutable_adt_false_when_not_marked() {
    let (frozen, (adt_id, adt_ty)) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let name_field = ctx.resolver().intern("value");
        let adt_id = AdtId::from_raw(301);
        let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
        fields.push(FieldDef {
            name: name_field,
            ty: i32_ty,
        });

        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields,
            variants: vec![VariantDef {
                name: name_field,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };

        ctx.register_adt(adt_id, adt_def);
        // Do NOT mark as interior mutable
        let substs = ctx.intern_substitution(vec![]);
        let adt_ty = ctx.mk_adt(adt_id, substs);
        (adt_id, adt_ty)
    });

    assert!(
        !frozen.is_interior_mutable_adt(adt_id),
        "unmarked ADT should not be interior mutable"
    );
    let flags = frozen.ty_flags(adt_ty);
    assert!(
        !flags.contains(TypeFlags::HAS_INTERIOR_MUTABILITY),
        "unmarked ADT should NOT have HAS_INTERIOR_MUTABILITY flag"
    );
}
