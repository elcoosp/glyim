use crate::const_val::Const;
use crate::*;
use glyim_core::def_id::AdtId;
use glyim_core::interner::Interner;
use glyim_core::primitives::*;

#[test]
fn test_is_copy_for_primitives() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let bool_ty = ctx.bool_ty();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let char_ty = ctx.mk_ty(TyKind::Char);

    assert!(ctx.is_copy(bool_ty));
    assert!(ctx.is_copy(i32_ty));
    assert!(ctx.is_copy(char_ty));
}

#[test]
fn test_is_copy_for_refs_is_false() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let inner = ctx.bool_ty();
    let ref_ty = ctx.mk_ref(Region::Erased, inner, Mutability::Not);

    assert!(!ctx.is_copy(ref_ty));
}

#[test]
fn test_is_copy_for_tuple_of_copy() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let bool_ty = ctx.bool_ty();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(bool_ty), GenericArg::Ty(i32_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(substs));

    assert!(ctx.is_copy(tuple_ty));
}

#[test]
fn test_is_sized_for_primitives_and_slices() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
    let arr_ty = ctx.mk_ty(TyKind::Array(
        i32_ty,
        Const {
            kind: ConstKind::Uint(3),
            ty: usize_ty,
        },
    ));
    let slice_ty = ctx.mk_ty(TyKind::Slice(i32_ty));

    let frozen = ctx.freeze();
    assert!(frozen.is_sized(i32_ty), "i32 is Sized");
    assert!(frozen.is_sized(arr_ty), "[i32; 3] is Sized");
    assert!(!frozen.is_sized(slice_ty), "[i32] is NOT Sized");
}

#[test]
fn test_deref_ty_for_refs_and_pointers() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);
    let ref_mut_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Mut);
    let raw_ty = ctx.mk_ty(TyKind::RawPtr(i32_ty, Mutability::Not));

    assert_eq!(ctx.deref_ty(ref_ty), Some(i32_ty), "&T derefs to T");
    assert_eq!(ctx.deref_ty(ref_mut_ty), Some(i32_ty), "&mut T derefs to T");
    assert_eq!(ctx.deref_ty(raw_ty), Some(i32_ty), "*const T derefs to T");
    assert_eq!(ctx.deref_ty(i32_ty), None, "i32 does not deref");
}

#[test]
fn test_deref_ty_consults_deref_impl_registry() {
    // Phase 5 (GLYIM_DESTUB_PLAN): a user `impl Deref for Box<T> { type Target =
    // T; }` must make `deref_ty(Box<T>)` return `T` (not None), so autoderef
    // can step through ADT Deref impls, not just structural &T / *T.
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    // Self type = a Box-like ADT parameterized by [T]; Target = T.
    let box_adt = AdtId::from_raw(1);
    let empty_substs = ctx.intern_substitution(vec![]);
    let box_i32_ty = ctx.mk_adt(box_adt, empty_substs);

    // Register `impl Deref for Box<_> { type Target = i32; }`.
    ctx.register_deref_impl(box_i32_ty, i32_ty);

    // An unregistered ADT (a fresh ADT id with no Deref impl) must deref to None.
    let other_adt = AdtId::from_raw(2);
    let other_ty = ctx.mk_adt(other_adt, empty_substs);

    assert_eq!(
        ctx.deref_ty(box_i32_ty),
        Some(i32_ty),
        "registered Deref impl must be reachable via deref_ty"
    );
    assert_eq!(
        ctx.deref_ty(other_ty),
        None,
        "unregistered ADT must deref to None"
    );

    // The registry survives a freeze (deref_ty on the frozen &TyCtx).
    let frozen = ctx.freeze();
    assert_eq!(
        frozen.deref_ty(box_i32_ty),
        Some(i32_ty),
        "deref registry must survive freeze"
    );
    assert_eq!(
        frozen.deref_ty(other_ty),
        None,
        "unregistered ADT must still deref to None after freeze"
    );
}
