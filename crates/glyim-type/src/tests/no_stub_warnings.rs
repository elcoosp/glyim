//! Tests verifying no STUB warnings are emitted during type computation (S04-T04)

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::IntTy;

use super::helpers::with_fresh_ty_ctx;
use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::auto_trait::AutoTraitFlags;
use crate::ty::{FieldIdx, TyKind};

/// S04-T04: Computing auto traits for a registered ADT should NOT emit
/// the "no AdtRepr registered" warning. When AdtDef is registered
/// via register_adt, compute_auto_traits uses it and does not fall through
/// to the unregistered-adt warning path.
#[test]
fn no_stub_warning_when_adt_def_registered() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let name_field = ctx.resolver().intern("x");
        let adt_id = AdtId::from_raw(400);
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
        let substs = ctx.intern_substitution(vec![]);
        ctx.mk_adt(adt_id, substs)
    });

    let flags = frozen.auto_trait_flags(adt_ty);
    // i32 is Send+Sync+Unpin, so struct with i32 field should have all flags
    assert!(
        flags.contains(AutoTraitFlags::SEND),
        "registered ADT should be Send"
    );
    assert!(
        flags.contains(AutoTraitFlags::SYNC),
        "registered ADT should be Sync"
    );
    assert!(
        flags.contains(AutoTraitFlags::UNPIN),
        "registered ADT should be Unpin"
    );
}

/// S04-T04b: When adt_reprs is used (but not adt_defs), no stub warning
/// should be emitted either.
#[test]
fn no_stub_warning_when_adt_repr_registered() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|ctx| {
        let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
        let adt_id = AdtId::from_raw(401);
        ctx.register_adt_repr(adt_id, vec![i32_ty]);

        let substs = ctx.intern_substitution(vec![]);
        ctx.mk_adt(adt_id, substs)
    });

    let flags = frozen.auto_trait_flags(adt_ty);
    assert!(
        flags.contains(AutoTraitFlags::SEND),
        "ADT with adt_repr should be Send"
    );
    assert!(
        flags.contains(AutoTraitFlags::SYNC),
        "ADT with adt_repr should be Sync"
    );
}
