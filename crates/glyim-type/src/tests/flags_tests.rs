use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::auto_trait::AutoTraitFlags;
use crate::region::Region;
use crate::ty::TyKind;
use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{IntTy, Mutability};

fn fresh_adt_id(raw: u32) -> AdtId {
    AdtId::from_raw(raw)
}

fn with_test_ctx<F, R>(f: F) -> (TyCtx, R)
where
    F: FnOnce(&mut TyCtxMut) -> R,
{
    let interner = Interner::default();
    let mut ctx_mut = TyCtxMut::new(interner);
    let result = f(&mut ctx_mut);
    (ctx_mut.freeze(), result)
}

#[test]
fn mut_ref_send_if_inner_send() {
    let (ctx, ty) = with_test_ctx(|ctx_mut| {
        let inner = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let region = Region::Erased;
        ctx_mut.mk_ref(region, inner, Mutability::Mut)
    });
    let auto_flags = ctx.auto_trait_flags(ty);
    assert!(
        auto_flags.contains(AutoTraitFlags::SEND),
        "&mut i32 should be Send"
    );
    assert!(
        auto_flags.contains(AutoTraitFlags::UNPIN),
        "&mut i32 should be Unpin"
    );
}

#[test]
fn field_ty_returns_correct_field_type() {
    let (ctx, field_tys) = with_test_ctx(|ctx_mut| {
        let adt_id = fresh_adt_id(200);
        let field1_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
        let field2_ty = ctx_mut.mk_ty(TyKind::Bool);

        let interner = Interner::default();
        let mut fields = IndexVec::new();
        fields.push(FieldDef {
            name: interner.intern("a"),
            ty: field1_ty,
        });
        fields.push(FieldDef {
            name: interner.intern("b"),
            ty: field2_ty,
        });
        let variant = VariantDef {
            name: interner.intern("MyStruct"),
            style: crate::adt_def::VariantStyle::Unit,
            fields: fields.clone(),
        };
        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields,
            variants: vec![variant],
            generic_params: vec![],
        };
        ctx_mut.register_adt(adt_id, adt_def);

        let ty0 = ctx_mut.field_ty(adt_id, 0);
        let ty1 = ctx_mut.field_ty(adt_id, 1);
        (ty0, ty1)
    });
    assert_eq!(ctx.ty_kind(field_tys.0), &TyKind::Int(IntTy::I32));
    assert_eq!(ctx.ty_kind(field_tys.1), &TyKind::Bool);
}
