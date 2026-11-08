use glyim_core::def_id::{AdtId, ClosureId, FnDefId, OpaqueTyId, TraitDefId};
use glyim_core::primitives::{Abi, FloatTy, IntTy, Mutability, Safety, UintTy};

use super::helpers::{test_frozen_ty_ctx, with_fresh_ty_ctx};
use crate::auto_trait::{AutoTrait, AutoTraitFlags};
use crate::const_val::{Const, ConstKind};
use crate::fn_sig::FnSig;
use crate::region::Region;
use crate::substitution::GenericArg;
use crate::ty::TyKind;
use crate::*;

/// Helper: create an ADT with given field types, returning (`frozen_ctx`, `adt_ty`)
fn make_single_field_adt(field_ty: Ty) -> (TyCtx, Ty) {
    with_fresh_ty_ctx(|ctx: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(100);
        let substs = ctx.intern_substitution(vec![]);
        ctx.register_adt_repr(adt_id, vec![field_ty]);
        ctx.mk_adt(adt_id, substs)
    })
}

// ===================== V06 Core Test Plan =====================

/// V06-T01: Struct with all Send fields → auto impl Send
#[test]
fn struct_all_send_fields_auto_impl_send() {
    let (ctx, adt_ty) = make_single_field_adt(ctx_bool_ty());
    let flags = ctx.auto_trait_flags(adt_ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

/// V06-T01 extended: struct with i32 field → all auto traits
#[test]
fn struct_with_int_field_auto_impls_all() {
    let (ctx, adt_ty) = make_single_field_adt(ctx_i32_ty());
    let flags = ctx.auto_trait_flags(adt_ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

/// V06-T02: Struct with non-Send field → no auto Send
#[test]
fn struct_with_non_send_field_no_auto_send() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let inner = ctx_mut.bool_ty();
        let raw_ptr_ty = ctx_mut.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        let adt_id = AdtId::from_raw(102);
        let substs = ctx_mut.intern_substitution(vec![]);
        ctx_mut.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        ctx_mut.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(adt_ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

/// V06-T03: Negative impl `impl !Send for T` → overrides auto
#[test]
fn negative_impl_overrides_auto_send() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(103);
        let substs = ctx_mut.intern_substitution(vec![]);
        let field_ty = ctx_mut.bool_ty();
        ctx_mut.register_adt_repr(adt_id, vec![field_ty]);
        ctx_mut.register_negative_impl(adt_id, AutoTrait::Send);
        ctx_mut.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(adt_ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

/// V06-T04: Transitive auto traits
#[test]
fn ref_send_requires_inner_sync() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let inner = ctx_mut.bool_ty();
        ctx_mut.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    assert!(ctx.auto_trait_flags(ref_ty).contains(AutoTraitFlags::SEND));
    assert!(ctx.auto_trait_flags(ref_ty).contains(AutoTraitFlags::SYNC));
}

#[test]
fn ref_mut_send_requires_inner_send() {
    let (ctx, ref_mut_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let inner = ctx_mut.bool_ty();
        ctx_mut.mk_ref(Region::Erased, inner, Mutability::Mut)
    });
    assert!(
        ctx.auto_trait_flags(ref_mut_ty)
            .contains(AutoTraitFlags::SEND)
    );
    assert!(
        ctx.auto_trait_flags(ref_mut_ty)
            .contains(AutoTraitFlags::SYNC)
    );
}

#[test]
fn ref_to_non_sync_is_not_send() {
    let (ctx, ref_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let inner_bool = ctx_mut.bool_ty();
        let raw_ptr_ty = ctx_mut.mk_ty(TyKind::RawPtr(inner_bool, Mutability::Not));
        let adt_id = AdtId::from_raw(104);
        let substs = ctx_mut.intern_substitution(vec![]);
        ctx_mut.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        let non_sync_ty = ctx_mut.mk_adt(adt_id, substs);
        ctx_mut.mk_ref(Region::Erased, non_sync_ty, Mutability::Not)
    });
    assert!(!ctx.auto_trait_flags(ref_ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(ref_ty).contains(AutoTraitFlags::SYNC));
}

/// V06-T05: Manual impl of auto trait overrides auto
#[test]
fn manual_impl_overrides_auto() {
    let (ctx, adt_ty) = with_fresh_ty_ctx(|ctx_mut: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(105);
        let substs = ctx_mut.intern_substitution(vec![]);
        let inner = ctx_mut.bool_ty();
        let raw_ptr_ty = ctx_mut.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        ctx_mut.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        ctx_mut.register_manual_impl(adt_id, AutoTrait::Send);
        ctx_mut.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(adt_ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(adt_ty).contains(AutoTraitFlags::SYNC));
}

// ===================== Primitives =====================

#[test]
fn primitives_have_all_auto_traits() {
    let ctx = test_frozen_ty_ctx();
    assert!(
        ctx.auto_trait_flags(Ty::BOOL)
            .contains(AutoTraitFlags::all())
    );
    assert!(
        ctx.auto_trait_flags(Ty::NEVER)
            .contains(AutoTraitFlags::all())
    );
    assert!(
        ctx.auto_trait_flags(Ty::UNIT)
            .contains(AutoTraitFlags::all())
    );
}

#[test]
fn int_types_have_all_auto_traits() {
    let (ctx, i32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(ctx.auto_trait_flags(i32_ty).contains(AutoTraitFlags::all()));
    let (ctx2, u64_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Uint(UintTy::U64)));
    assert!(
        ctx2.auto_trait_flags(u64_ty)
            .contains(AutoTraitFlags::all())
    );
}

#[test]
fn char_type_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Char));
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn string_type_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::String));
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn float_types_have_all_auto_traits() {
    let (ctx, f32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Float(FloatTy::F32)));
    assert!(ctx.auto_trait_flags(f32_ty).contains(AutoTraitFlags::all()));
    let (ctx2, f64_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Float(FloatTy::F64)));
    assert!(
        ctx2.auto_trait_flags(f64_ty)
            .contains(AutoTraitFlags::all())
    );
}

#[test]
fn never_type_has_all_auto_traits() {
    let ctx = test_frozen_ty_ctx();
    assert!(
        ctx.auto_trait_flags(Ty::NEVER)
            .contains(AutoTraitFlags::all())
    );
}

// ===================== Error / Infer / Param / Bound =====================

#[test]
fn error_type_has_no_auto_traits() {
    let ctx = test_frozen_ty_ctx();
    let flags = ctx.auto_trait_flags(Ty::ERROR);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn infer_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        c.mk_ty(TyKind::Infer(InferVar::Ty(TyVar::from_raw(0))))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn param_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let name = c.resolver().intern("T");
        c.mk_ty(TyKind::Param(ParamTy { index: 0, name }))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn bound_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        c.mk_ty(TyKind::Bound(
            0,
            BoundTy {
                var: 0,
                kind: BoundTyKind::Anon,
            },
        ))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

// ===================== Raw pointers =====================

#[test]
fn raw_ptr_is_unpin_only() {
    let (ctx, ty) =
        with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not)));
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));

    let (ctx2, ty2) =
        with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Mut)));
    let flags2 = ctx2.auto_trait_flags(ty2);
    assert!(!flags2.contains(AutoTraitFlags::SEND));
    assert!(!flags2.contains(AutoTraitFlags::SYNC));
    assert!(flags2.contains(AutoTraitFlags::UNPIN));
}

// ===================== Tuples =====================

#[test]
fn tuple_auto_traits_are_intersection() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(bool_ty, Mutability::Not));
        let substs =
            c.intern_substitution(vec![GenericArg::Ty(bool_ty), GenericArg::Ty(raw_ptr_ty)]);
        c.mk_tuple(substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn empty_tuple_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![]);
        c.mk_tuple(substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn tuple_with_unit() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![
            GenericArg::Ty(c.unit_ty()),
            GenericArg::Ty(c.bool_ty()),
        ]);
        c.mk_tuple(substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn tuple_with_never() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![
            GenericArg::Ty(c.never_ty()),
            GenericArg::Ty(c.bool_ty()),
        ]);
        c.mk_tuple(substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn tuple_of_many_send_types() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u64_ty = c.mk_ty(TyKind::Uint(UintTy::U64));
        let f64_ty = c.mk_ty(TyKind::Float(FloatTy::F64));
        let substs = c.intern_substitution(vec![
            GenericArg::Ty(bool_ty),
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(u64_ty),
            GenericArg::Ty(f64_ty),
        ]);
        c.mk_tuple(substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

// ===================== Slice / Array =====================

#[test]
fn slice_inherits_element_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Slice(c.bool_ty())));
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn slice_of_raw_ptr_is_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        c.mk_ty(TyKind::Slice(raw_ptr_ty))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn array_inherits_element_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        c.mk_ty(TyKind::Array(
            bool_ty,
            Const {
                kind: ConstKind::Uint(3),
                ty: bool_ty,
            },
        ))
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn array_of_raw_ptr_is_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(bool_ty, Mutability::Not));
        c.mk_ty(TyKind::Array(
            raw_ptr_ty,
            Const {
                kind: ConstKind::Uint(3),
                ty: bool_ty,
            },
        ))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn array_of_send_adt_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let adt_id = AdtId::from_raw(880);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![bool_ty]);
        let adt_ty = c.mk_adt(adt_id, substs);
        c.mk_ty(TyKind::Array(
            adt_ty,
            Const {
                kind: ConstKind::Uint(4),
                ty: bool_ty,
            },
        ))
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn nested_slice_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner_slice = c.mk_ty(TyKind::Slice(c.bool_ty()));
        c.mk_ty(TyKind::Slice(inner_slice))
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

// ===================== ADT =====================

#[test]
fn multi_field_adt_all_must_implement() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let adt_id = AdtId::from_raw(106);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![bool_ty, i32_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn multi_field_adt_one_bad_field_removes_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(bool_ty, Mutability::Not));
        let adt_id = AdtId::from_raw(107);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![bool_ty, raw_ptr_ty]);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn adt_with_no_repr_gets_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(500);
        let substs = c.intern_substitution(vec![]);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn adt_with_empty_fields_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(501);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_ref_field_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let ref_ty = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not);
        let adt_id = AdtId::from_raw(830);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![ref_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_ref_mut_field_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let ref_mut_ty = c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Mut);
        let adt_id = AdtId::from_raw(831);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![ref_mut_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_error_field_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(893);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![Ty::ERROR]);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn adt_with_many_send_fields() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let i8_ty = c.mk_ty(TyKind::Int(IntTy::I8));
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let f32_ty = c.mk_ty(TyKind::Float(FloatTy::F32));
        let adt_id = AdtId::from_raw(800);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![bool_ty, i8_ty, i32_ty, u32_ty, f32_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_tuple_field() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let tuple_substs =
            c.intern_substitution(vec![GenericArg::Ty(bool_ty), GenericArg::Ty(i32_ty)]);
        let tuple_ty = c.mk_tuple(tuple_substs);
        let adt_id = AdtId::from_raw(890);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![tuple_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_slice_field() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let slice_ty = c.mk_ty(TyKind::Slice(c.bool_ty()));
        let adt_id = AdtId::from_raw(891);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![slice_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn adt_with_fn_ptr_field() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let inputs = c.intern_substitution(vec![GenericArg::Ty(bool_ty)]);
        let sig = FnSig {
            inputs,
            output: bool_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_ptr_ty = c.mk_fn_ptr(sig);
        let adt_id = AdtId::from_raw(892);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![fn_ptr_ty]);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

// ===================== Negative impls =====================

#[test]
fn negative_impl_send_doesnt_affect_sync_or_unpin() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(108);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn negative_impl_sync_doesnt_affect_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(300);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Sync);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn negative_impl_unpin_doesnt_affect_send_or_sync() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(301);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Unpin);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(!flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn multiple_negative_impls() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(302);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        c.register_negative_impl(adt_id, AutoTrait::Sync);
        c.mk_adt(adt_id, substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn all_auto_traits_negative() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(860);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        c.register_negative_impl(adt_id, AutoTrait::Sync);
        c.register_negative_impl(adt_id, AutoTrait::Unpin);
        c.mk_adt(adt_id, substs)
    });
    assert_eq!(ctx.auto_trait_flags(ty), AutoTraitFlags::empty());
}

// ===================== Manual impls =====================

#[test]
fn manual_impl_sync_on_raw_ptr_struct() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(303);
        let substs = c.intern_substitution(vec![]);
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        c.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        c.register_manual_impl(adt_id, AutoTrait::Sync);
        c.mk_adt(adt_id, substs)
    });
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SYNC));
}

#[test]
fn manual_impl_overrides_negative_impl() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(304);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        c.register_manual_impl(adt_id, AutoTrait::Send);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
}

#[test]
fn manual_impl_all_traits_on_raw_ptr_struct() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(870);
        let substs = c.intern_substitution(vec![]);
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        c.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        c.register_manual_impl(adt_id, AutoTrait::Send);
        c.register_manual_impl(adt_id, AutoTrait::Sync);
        c.register_manual_impl(adt_id, AutoTrait::Unpin);
        c.mk_adt(adt_id, substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

// ===================== Nested ADTs =====================

#[test]
fn nested_adt_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner_adt_id = AdtId::from_raw(400);
        let inner_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(inner_adt_id, vec![c.bool_ty()]);
        let inner_ty = c.mk_adt(inner_adt_id, inner_substs);
        let outer_adt_id = AdtId::from_raw(401);
        let outer_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(outer_adt_id, vec![inner_ty]);
        c.mk_adt(outer_adt_id, outer_substs)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn nested_adt_inner_not_send_outer_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        let inner_adt_id = AdtId::from_raw(402);
        let inner_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(inner_adt_id, vec![raw_ptr_ty]);
        let inner_ty = c.mk_adt(inner_adt_id, inner_substs);
        let outer_adt_id = AdtId::from_raw(403);
        let outer_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(outer_adt_id, vec![inner_ty]);
        c.mk_adt(outer_adt_id, outer_substs)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn deeply_nested_adts() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id_1 = AdtId::from_raw(810);
        c.register_adt_repr(adt_id_1, vec![c.bool_ty()]);
        let substs_1 = c.intern_substitution(vec![]);
        let ty_1 = c.mk_adt(adt_id_1, substs_1);
        let adt_id_2 = AdtId::from_raw(811);
        c.register_adt_repr(adt_id_2, vec![ty_1]);
        let substs_2 = c.intern_substitution(vec![]);
        let ty_2 = c.mk_adt(adt_id_2, substs_2);
        let adt_id_3 = AdtId::from_raw(812);
        c.register_adt_repr(adt_id_3, vec![ty_2]);
        let substs_3 = c.intern_substitution(vec![]);
        c.mk_adt(adt_id_3, substs_3)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn deeply_nested_adts_non_send_inner() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        let adt_id_1 = AdtId::from_raw(820);
        c.register_adt_repr(adt_id_1, vec![raw_ptr_ty]);
        let substs_1 = c.intern_substitution(vec![]);
        let ty_1 = c.mk_adt(adt_id_1, substs_1);
        let adt_id_2 = AdtId::from_raw(821);
        c.register_adt_repr(adt_id_2, vec![ty_1]);
        let substs_2 = c.intern_substitution(vec![]);
        c.mk_adt(adt_id_2, substs_2)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
}

// ===================== Coinductive =====================

#[test]
fn coinductive_recursive_adt() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(200);
        let substs = c.intern_substitution(vec![]);
        let adt_ty = c.mk_adt(adt_id, substs);
        c.register_adt_repr(adt_id, vec![adt_ty]);
        adt_ty
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn coinductive_mutual_recursion() {
    let (ctx, ty_a) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id_a = AdtId::from_raw(900);
        let adt_id_b = AdtId::from_raw(901);
        let substs_a = c.intern_substitution(vec![]);
        let substs_b = c.intern_substitution(vec![]);
        let ty_a = c.mk_adt(adt_id_a, substs_a);
        let ty_b = c.mk_adt(adt_id_b, substs_b);
        c.register_adt_repr(adt_id_a, vec![ty_b]);
        c.register_adt_repr(adt_id_b, vec![ty_a]);
        ty_a
    });
    let flags = ctx.auto_trait_flags(ty_a);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

// ===================== FnPtr / FnDef / Closure =====================

#[test]
fn fn_ptr_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let inputs = c.intern_substitution(vec![GenericArg::Ty(bool_ty)]);
        let sig = FnSig {
            inputs,
            output: bool_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        c.mk_fn_ptr(sig)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn fn_def_has_all_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::FnDef(FnDefId::from_raw(0), substs))
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn closure_with_all_send_fields_is_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(0), substs))
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn closure_with_raw_ptr_is_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        let substs = c.intern_substitution(vec![GenericArg::Ty(raw_ptr_ty)]);
        c.mk_ty(TyKind::Closure(ClosureId::from_raw(1), substs))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

// ===================== Dynamic / Opaque / Projection =====================

#[test]
fn dynamic_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let binder = Binder::bind(Box::new([]) as Box<[Predicate]>, Box::new([]));
        c.mk_ty(TyKind::Dynamic(binder, Region::Static))
    });
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SYNC));
}

#[test]
fn opaque_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(0), substs))
    });
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SYNC));
}

/// Plan §7.2: an opaque type (`impl Trait` return position) must delegate its
/// auto-trait set to its *defining-use* concrete (hidden) type rather than
/// assuming zero auto traits. Here the hidden type is a `*const i32`, which is
/// `Unpin` only (not `Send`/`Sync`), so the opaque must be `Unpin` only too.
#[test]
fn opaque_type_delegates_to_registered_hidden_type() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.mk_ty(TyKind::Int(IntTy::I32));
        let hidden = c.mk_ty(TyKind::RawPtr(inner, Mutability::Not));
        c.register_opaque_hidden(OpaqueTyId::from_raw(0), hidden);
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(0), substs))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

/// Plan §7.2: when the hidden type is `Send`/`Sync` (here `i32`), the opaque
/// must report `Send`/`Sync` instead of the old all-empty behaviour.
#[test]
fn opaque_type_inherits_send_from_hidden_type() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let hidden = c.mk_ty(TyKind::Int(IntTy::I32));
        c.register_opaque_hidden(OpaqueTyId::from_raw(1), hidden);
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Opaque(OpaqueTyId::from_raw(1), substs))
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(flags.contains(AutoTraitFlags::SEND));
    assert!(flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn projection_type_has_no_auto_traits() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let trait_ref = TraitRef {
            def_id: TraitDefId::from_raw(0),
            substs,
        };
        let name = c.resolver().intern("Item");
        c.mk_ty(TyKind::Projection(ProjectionTy {
            trait_ref,
            item_name: name,
        }))
    });
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SYNC));
}

// ===================== Ref edge cases =====================

#[test]
fn ref_mut_to_non_sync_is_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        let adt_id = AdtId::from_raw(404);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        let non_sync_ty = c.mk_adt(adt_id, substs);
        c.mk_ref(Region::Erased, non_sync_ty, Mutability::Mut)
    });
    let flags = ctx.auto_trait_flags(ty);
    assert!(!flags.contains(AutoTraitFlags::SEND));
    assert!(!flags.contains(AutoTraitFlags::SYNC));
    assert!(flags.contains(AutoTraitFlags::UNPIN));
}

#[test]
fn ref_to_send_but_not_sync_is_not_send() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(c.bool_ty(), Mutability::Not));
        let adt_id = AdtId::from_raw(405);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![raw_ptr_ty]);
        c.register_manual_impl(adt_id, AutoTrait::Send);
        let send_but_not_sync = c.mk_adt(adt_id, substs);
        c.mk_ref(Region::Erased, send_but_not_sync, Mutability::Not)
    });
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(ty).contains(AutoTraitFlags::SYNC));
}

#[test]
fn ref_with_static_region() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        c.mk_ref(Region::Static, c.bool_ty(), Mutability::Not)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

#[test]
fn ref_with_erased_region() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });
    assert!(ctx.auto_trait_flags(ty).contains(AutoTraitFlags::all()));
}

// ===================== Convenience methods =====================

#[test]
fn implements_auto_trait_convenience_method() {
    let ctx = test_frozen_ty_ctx();
    assert!(ctx.implements_auto_trait(Ty::BOOL, AutoTrait::Send));
    assert!(ctx.implements_auto_trait(Ty::BOOL, AutoTrait::Sync));
    assert!(ctx.implements_auto_trait(Ty::BOOL, AutoTrait::Unpin));
    assert!(!ctx.implements_auto_trait(Ty::ERROR, AutoTrait::Send));
}

#[test]
fn frozen_context_negative_and_manual_impl_queries() {
    let (ctx, ()) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(600);
        c.register_negative_impl(adt_id, AutoTrait::Send);
        c.register_manual_impl(adt_id, AutoTrait::Sync);
    });
    let adt_id = AdtId::from_raw(600);
    assert!(ctx.has_negative_impl(adt_id, AutoTrait::Send));
    assert!(!ctx.has_negative_impl(adt_id, AutoTrait::Sync));
    assert!(ctx.has_manual_impl(adt_id, AutoTrait::Sync));
    assert!(!ctx.has_manual_impl(adt_id, AutoTrait::Send));
}

#[test]
fn frozen_context_adt_repr_accessor() {
    let (ctx, ()) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(700);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
    });
    let adt_id = AdtId::from_raw(700);
    assert!(ctx.adt_repr(adt_id).is_some());
    assert_eq!(ctx.adt_repr(adt_id).unwrap().field_tys.len(), 1);
    assert!(ctx.adt_repr(AdtId::from_raw(999)).is_none());
}

#[test]
fn auto_trait_flag_mapping() {
    assert_eq!(AutoTrait::Send.flag(), AutoTraitFlags::SEND);
    assert_eq!(AutoTrait::Sync.flag(), AutoTraitFlags::SYNC);
    assert_eq!(AutoTrait::Unpin.flag(), AutoTraitFlags::UNPIN);
}

#[test]
fn auto_trait_all_constant() {
    assert_eq!(AutoTrait::ALL.len(), 3);
    assert!(AutoTrait::ALL.contains(&AutoTrait::Send));
    assert!(AutoTrait::ALL.contains(&AutoTrait::Sync));
    assert!(AutoTrait::ALL.contains(&AutoTrait::Unpin));
}

#[test]
fn auto_trait_flags_combinations() {
    let send_only = AutoTraitFlags::SEND;
    assert!(send_only.contains(AutoTraitFlags::SEND));
    assert!(!send_only.contains(AutoTraitFlags::SYNC));
    let send_sync = AutoTraitFlags::SEND | AutoTraitFlags::SYNC;
    assert!(send_sync.contains(AutoTraitFlags::SEND));
    assert!(send_sync.contains(AutoTraitFlags::SYNC));
    assert!(!send_sync.contains(AutoTraitFlags::UNPIN));
    let all = AutoTraitFlags::all();
    assert!(
        all.contains(AutoTraitFlags::SEND)
            && all.contains(AutoTraitFlags::SYNC)
            && all.contains(AutoTraitFlags::UNPIN)
    );
    let empty = AutoTraitFlags::empty();
    assert!(!empty.contains(AutoTraitFlags::SEND));
}

#[test]
fn auto_trait_flags_are_cached() {
    let (ctx, ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let adt_id = AdtId::from_raw(840);
        let substs = c.intern_substitution(vec![]);
        c.register_adt_repr(adt_id, vec![c.bool_ty()]);
        c.mk_adt(adt_id, substs)
    });
    let flags1 = ctx.auto_trait_flags(ty);
    let flags2 = ctx.auto_trait_flags(ty);
    assert_eq!(flags1, flags2);
}

#[test]
fn multiple_adts_independent() {
    let (ctx, pair) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let raw_ptr_ty = c.mk_ty(TyKind::RawPtr(bool_ty, Mutability::Not));
        let send_adt_id = AdtId::from_raw(850);
        let send_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(send_adt_id, vec![bool_ty]);
        let send_adt = c.mk_adt(send_adt_id, send_substs);
        let nosend_adt_id = AdtId::from_raw(851);
        let nosend_substs = c.intern_substitution(vec![]);
        c.register_adt_repr(nosend_adt_id, vec![raw_ptr_ty]);
        let nosend_adt = c.mk_adt(nosend_adt_id, nosend_substs);
        (send_adt, nosend_adt)
    });
    assert!(ctx.auto_trait_flags(pair.0).contains(AutoTraitFlags::SEND));
    assert!(!ctx.auto_trait_flags(pair.1).contains(AutoTraitFlags::SEND));
}

// ===================== Helpers =====================

fn ctx_bool_ty() -> Ty {
    let ctx = super::helpers::test_ty_ctx();
    ctx.bool_ty()
}

fn ctx_i32_ty() -> Ty {
    let mut ctx = super::helpers::test_ty_ctx();
    ctx.mk_ty(TyKind::Int(IntTy::I32))
}
