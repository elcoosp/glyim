//! Tests for the unified `TyCtx::needs_drop` (de-stubbing plan §8.2/§12.3).
//!
//! `needs_drop` is the single authority consulted by both `glyim-lower` (MIR
//! building) and `glyim-opt` (drop elaboration). These tests pin down its
//! semantics and, crucially, assert the cases where the *old* divergent
//! implementations disagreed (e.g. unknown-ADT handling) now have one
//! unambiguous answer.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::AdtId;
use glyim_core::primitives::{IntTy, Mutability, UintTy};

use super::helpers::with_fresh_ty_ctx;
use crate::adt_def::{AdtDef, AdtKind, FieldDef, VariantDef};
use crate::const_val::{Const, ConstKind};
use crate::region::Region;
use crate::ty::{FieldIdx, TyKind};

#[test]
fn primitives_never_need_drop() {
    let (ctx, tys) = with_fresh_ty_ctx(|c| {
        (
            c.mk_ty(TyKind::Int(IntTy::I32)),
            c.mk_ty(TyKind::Uint(UintTy::U8)),
            c.mk_ty(TyKind::Bool),
            c.mk_ty(TyKind::Char),
            c.mk_ty(TyKind::Unit),
            c.mk_ty(TyKind::Never),
        )
    });
    assert!(!ctx.needs_drop(tys.0));
    assert!(!ctx.needs_drop(tys.1));
    assert!(!ctx.needs_drop(tys.2));
    assert!(!ctx.needs_drop(tys.3));
    assert!(!ctx.needs_drop(tys.4));
    assert!(!ctx.needs_drop(tys.5));
}

#[test]
fn references_and_raw_pointers_never_need_drop() {
    let (ctx, (r, raw)) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let r = c.mk_ref(Region::Static, i32_ty, Mutability::Not);
        let raw = c.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));
        (r, raw)
    });
    assert!(!ctx.needs_drop(r));
    assert!(!ctx.needs_drop(raw));
}

#[test]
fn struct_of_primitives_does_not_need_drop() {
    // Build the struct and its primitive field type in the SAME context so the
    // `Ty` indices stay valid (cross-context Ty usage would resolve to garbage).
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let name = c.resolver().intern("f");
        let mut fields: IndexVec<FieldIdx, FieldDef> = IndexVec::new();
        fields.push(FieldDef { name, ty: i32_ty });
        fields.push(FieldDef { name, ty: i32_ty });
        let adt_id = AdtId::from_raw(0);
        let adt_def = AdtDef {
            kind: AdtKind::Struct,
            fields: fields.clone(),
            variants: vec![VariantDef {
                name,
                fields,
                style: crate::adt_def::VariantStyle::Unit,
            }],
            generic_params: vec![],
        };
        c.register_adt(adt_id, adt_def);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(!ctx.needs_drop(ty));
}

#[test]
fn struct_containing_droppable_field_needs_drop() {
    // A struct whose field is itself a registered ADT that has a Drop impl.
    let (ctx, (inner_ty, outer_ty)) = with_fresh_ty_ctx(|c| {
        // Inner "owning" type (e.g. a simplified String): mark it as having Drop.
        let inner_id = AdtId::from_raw(1);
        let f = c.resolver().intern("f");
        let inner_def = AdtDef {
            kind: AdtKind::Struct,
            fields: IndexVec::new(),
            variants: vec![VariantDef {
                name: f,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };
        c.register_adt(inner_id, inner_def);
        c.mark_has_drop(inner_id);
        let inner_substs = c.intern_substitution(vec![]);
        let inner_ty = c.mk_adt(inner_id, inner_substs);

        // Outer struct holding the inner type.
        let outer_id = AdtId::from_raw(2);
        let mut of = IndexVec::new();
        of.push(FieldDef {
            name: f,
            ty: inner_ty,
        });
        let outer_def = AdtDef {
            kind: AdtKind::Struct,
            fields: of.clone(),
            variants: vec![VariantDef {
                name: f,
                fields: of,
                style: crate::adt_def::VariantStyle::Unit,
            }],
            generic_params: vec![],
        };
        c.register_adt(outer_id, outer_def);
        let outer_substs = c.intern_substitution(vec![]);
        let outer_ty = c.mk_adt(outer_id, outer_substs);
        (inner_ty, outer_ty)
    });
    assert!(ctx.needs_drop(inner_ty), "Drop-impl ADT must need drop");
    assert!(
        ctx.needs_drop(outer_ty),
        "struct containing a Drop-impl field must need drop"
    );
}

#[test]
fn array_and_slice_of_droppable_need_drop() {
    let (ctx, (arr, slice)) = with_fresh_ty_ctx(|c| {
        let id = AdtId::from_raw(5);
        let f = c.resolver().intern("f");
        let def = AdtDef {
            kind: AdtKind::Struct,
            fields: IndexVec::new(),
            variants: vec![VariantDef {
                name: f,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };
        c.register_adt(id, def);
        c.mark_has_drop(id);
        let substs = c.intern_substitution(vec![]);
        let inner_ty = c.mk_adt(id, substs);
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let arr = c.mk_ty(TyKind::Array(
            inner_ty,
            Const {
                kind: ConstKind::Uint(3),
                ty: usize_ty,
            },
        ));
        let slice = c.mk_ty(TyKind::Slice(inner_ty));
        (arr, slice)
    });
    assert!(ctx.needs_drop(arr));
    assert!(ctx.needs_drop(slice));
}

#[test]
fn string_owns_and_needs_drop() {
    // `String` is an owning builtin represented as a standalone `TyKind`; it
    // must need drop even though it carries no embedded `AdtId`. This is the
    // case the old `glyim-lower` hardcoded and that the unified function must
    // preserve (plan §8.2).
    let (ctx, ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::String));
    assert!(ctx.needs_drop(ty));
}

#[test]
fn tuple_of_primitives_does_not_need_drop() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let i = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs =
            c.intern_substitution(vec![crate::GenericArg::Ty(i), crate::GenericArg::Ty(i)]);
        c.mk_ty(TyKind::Tuple(substs))
    });
    assert!(!ctx.needs_drop(ty));
}

#[test]
fn union_always_needs_drop() {
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let id = AdtId::from_raw(9);
        let f = c.resolver().intern("f");
        let def = AdtDef {
            kind: AdtKind::Union,
            fields: IndexVec::new(),
            variants: vec![VariantDef {
                name: f,
                style: crate::adt_def::VariantStyle::Unit,
                fields: IndexVec::new(),
            }],
            generic_params: vec![],
        };
        c.register_adt(id, def);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(id, substs)
    });
    assert!(
        ctx.needs_drop(ty),
        "union must need drop (user owns active variant)"
    );
}

#[test]
fn unknown_unregistered_adt_does_not_need_drop() {
    // An Adt whose id was never registered. Previously `glyim-opt` returned
    // `true` (conservative) for this case while `glyim-lower` returned `false`
    // — the divergence this unification closes. The agreed answer is `false`
    // (no spurious destructors for unmodellable types).
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        let id = AdtId::from_raw(999); // never registered
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(id, substs)
    });
    assert!(!ctx.needs_drop(ty));
}

#[test]
fn dyn_and_generic_param_do_not_need_drop() {
    // Types the model cannot inspect must not spuriously need drop.
    let (ctx, ty) = with_fresh_ty_ctx(|c| {
        c.mk_ty(TyKind::Param(crate::ty::ParamTy {
            index: 0,
            name: c.resolver().intern("T"),
        }))
    });
    assert!(!ctx.needs_drop(ty));
}
