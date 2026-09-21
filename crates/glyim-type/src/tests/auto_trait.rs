//! Tests for auto trait computation (S04-T01)

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::{IntTy, Mutability};

use super::helpers::with_fresh_ty_ctx;
use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::auto_trait::{AutoTrait, AutoTraitFlags};
use crate::ty::{FieldIdx, TyKind};

/// S04-T01: `Send` computed correctly for struct with a raw ptr field.
/// Raw pointers are !Send + !Sync, so a struct containing one loses those traits.
#[test]
fn send_computed_for_struct_with_raw_ptr_field() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let raw_ptr_ty = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));

        let name_field = ctx.resolver().intern("ptr_field");
        let adt_id = AdtId::from_raw(100);
        let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
        fields.push(FieldDef {
            name: name_field,
            ty: raw_ptr_ty,
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
        let substs = ctx.intern_substitution(vec![]);
        ctx.mk_adt(adt_id, substs)
    });

    let flags = frozen.auto_trait_flags(adt_ty);
    // Raw pointers lose Send and Sync
    assert!(
        !flags.contains(AutoTraitFlags::SEND),
        "struct with raw ptr field should not be Send"
    );
    assert!(
        !flags.contains(AutoTraitFlags::SYNC),
        "struct with raw ptr field should not be Sync"
    );
    // Unpin is always present for raw pointers
    assert!(
        flags.contains(AutoTraitFlags::UNPIN),
        "struct with raw ptr field should be Unpin"
    );
}

/// S04-T01b: A struct with all-primitive fields should have all auto traits.
#[test]
fn send_computed_for_struct_with_primitive_fields() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let bool_ty = ctx.bool_ty();

        let name_a = ctx.resolver().intern("a");
        let name_b = ctx.resolver().intern("b");
        let adt_id = AdtId::from_raw(101);
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
        ctx.mk_adt(adt_id, substs)
    });

    let flags = frozen.auto_trait_flags(adt_ty);
    assert!(
        flags.contains(AutoTraitFlags::SEND),
        "struct with primitive fields should be Send"
    );
    assert!(
        flags.contains(AutoTraitFlags::SYNC),
        "struct with primitive fields should be Sync"
    );
    assert!(
        flags.contains(AutoTraitFlags::UNPIN),
        "struct with primitive fields should be Unpin"
    );
}

/// S04-T01c: A struct with a negative impl for Send should not be Send.
#[test]
fn send_negative_impl_overrides_field_computation() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let name_field = ctx.resolver().intern("x");
        let adt_id = AdtId::from_raw(102);
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
        ctx.register_negative_impl(adt_id, AutoTrait::Send);
        let substs = ctx.intern_substitution(vec![]);
        ctx.mk_adt(adt_id, substs)
    });

    let flags = frozen.auto_trait_flags(adt_ty);
    assert!(
        !flags.contains(AutoTraitFlags::SEND),
        "struct with negative Send impl should not be Send"
    );
    assert!(
        flags.contains(AutoTraitFlags::SYNC),
        "struct with only negative Send impl should still be Sync"
    );
}
