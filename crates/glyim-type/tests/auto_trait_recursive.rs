//! Test coinductive auto-trait computation on mutually recursive types.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{IntTy, Mutability};
use glyim_type::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use glyim_type::{AutoTraitFlags, Ty, TyCtxMut};

fn build_ctx() -> TyCtxMut {
    TyCtxMut::new(Interner::new())
}

// Helper: build a struct ADT definition from field names and types.
fn mk_struct_def(ctx: &mut TyCtxMut, fields: Vec<(&str, Ty)>) -> AdtDef {
    let mut field_defs = IndexVec::new();
    let mut variant_defs = Vec::new();
    let mut variant_fields = IndexVec::new();
    for (name, ty) in fields {
        let name = ctx.resolver().intern(name);
        let fd = FieldDef { name, ty };
        field_defs.push(fd.clone());
        variant_fields.push(fd);
    }
    variant_defs.push(VariantDef {
        name: ctx.resolver().intern(""),
        style: glyim_type::adt_def::VariantStyle::Unit,
        fields: variant_fields,
    });
    AdtDef {
        kind: AdtKind::Struct,
        fields: field_defs,
        variants: variant_defs,
        generic_params: vec![],
    }
}

#[test]
fn mutually_recursive_send_sync() {
    let mut ctx_mut = build_ctx();
    let i32_ty = ctx_mut.mk_ty(glyim_type::TyKind::Int(IntTy::I32));

    // IDs for dummy and real ADTs.
    let id_a = AdtId::from_raw(2000);
    let id_b = AdtId::from_raw(2001);
    let id_a2 = AdtId::from_raw(2002);
    let id_b2 = AdtId::from_raw(2003);

    // 1. Register dummy ADTs.
    let dummy_def = mk_struct_def(&mut ctx_mut, vec![("data", i32_ty)]);
    ctx_mut.register_adt(id_a, dummy_def);
    let dummy_def_b = mk_struct_def(&mut ctx_mut, vec![("data", i32_ty)]);
    ctx_mut.register_adt(id_b, dummy_def_b);

    // 2. Compute empty substitution (no nested borrows).
    let empty_subst = ctx_mut.intern_substitution(vec![]);

    // 3. Build pointers to each other using the now-registered ADTs.
    let node_b_ty = ctx_mut.mk_ty(glyim_type::TyKind::Adt(id_b, empty_subst));
    let ptr_b_raw = ctx_mut.mk_ty(glyim_type::TyKind::RawPtr(node_b_ty, Mutability::Not));

    let node_a_ty = ctx_mut.mk_ty(glyim_type::TyKind::Adt(id_a, empty_subst));
    let ptr_a_raw = ctx_mut.mk_ty(glyim_type::TyKind::RawPtr(node_a_ty, Mutability::Not));

    // 4. Register real ADTs with new IDs.
    let real_a_def = mk_struct_def(&mut ctx_mut, vec![("next", ptr_b_raw), ("data", i32_ty)]);
    ctx_mut.register_adt(id_a2, real_a_def);

    let real_b_def = mk_struct_def(&mut ctx_mut, vec![("next", ptr_a_raw), ("data", i32_ty)]);
    ctx_mut.register_adt(id_b2, real_b_def);

    // 5. Get final Ty values.
    let ty_a2 = ctx_mut.mk_ty(glyim_type::TyKind::Adt(id_a2, empty_subst));
    let ty_b2 = ctx_mut.mk_ty(glyim_type::TyKind::Adt(id_b2, empty_subst));

    let ctx = ctx_mut.freeze();

    // 6. Assert auto-traits.
    let flags_a = ctx.auto_trait_flags(ty_a2);
    assert!(!flags_a.contains(AutoTraitFlags::SEND));
    assert!(!flags_a.contains(AutoTraitFlags::SYNC));
    assert!(flags_a.contains(AutoTraitFlags::UNPIN));

    let flags_b = ctx.auto_trait_flags(ty_b2);
    assert!(!flags_b.contains(AutoTraitFlags::SEND));
    assert!(!flags_b.contains(AutoTraitFlags::SYNC));
    assert!(flags_b.contains(AutoTraitFlags::UNPIN));
}
