use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Const, ConstKind, FieldIdx, GenericArg, Region, Ty, TyCtxMut, TyKind};

#[test]
fn ty_field_on_unit_tuple() {
    let (ctx, unit_tuple_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let substs = c.intern_substitution(vec![]);
        c.mk_ty(TyKind::Tuple(substs))
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: unit_tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_deref_on_unit_type() {
    let (ctx, unit_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.unit_ty());

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_deref_on_int_type() {
    let (ctx, i32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Int(IntTy::I32)));

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_field_on_ref_returns_error() {
    let (ctx, ref_bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(0))]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_index_on_tuple_returns_error() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        c.mk_ty(TyKind::Tuple(substs))
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_usize(1))]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_downcast_preserves_through_chain() {
    let (ctx, (tuple_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        (tuple_ty, i32_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Field(FieldIdx::from_usize(0)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_deref_raw_ptr_to_tuple_then_field() {
    let (ctx, (raw_ptr_tuple_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        let raw_ptr_tuple_ty = c.mk_ty(TyKind::RawPtr(tuple_ty, Mutability::Not));
        (raw_ptr_tuple_ty, i32_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: raw_ptr_tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_usize(0)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_multiple_locals() {
    let (ctx, (ref_i32_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let ref_i32_ty = c.mk_ref(Region::Erased, i32_ty, Mutability::Not);
        (ref_i32_ty, i32_ty)
    });

    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    locals.push(LocalDecl {
        ty: ref_i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place_local1 = Place {
        local: LocalIdx::from_usize(1),
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place_local1.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_ref_to_slice_then_deref_then_index() {
    let (ctx, (ref_slice_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = c.mk_ty(TyKind::Slice(i32_ty));
        let ref_slice_ty = c.mk_ref(Region::Erased, slice_ty, Mutability::Not);
        (ref_slice_ty, i32_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_slice_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Index(LocalIdx::from_usize(1)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_ref_to_array_then_deref_then_field_fails() {
    let (ctx, ref_array_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(4),
            ty: usize_ty,
        };
        let array_ty = c.mk_ty(TyKind::Array(bool_ty, len));
        c.mk_ref(Region::Erased, array_ty, Mutability::Not)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_usize(0)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_bool_local_no_projection() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place::new(local);
    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_error_local_no_projection() {
    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::ERROR,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let (ctx, _) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());
    let place = Place::new(local);
    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_deeply_nested_refs() {
    let (ctx, (triple_ref_bool, _bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let ref1 = c.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref2 = c.mk_ref(Region::Erased, ref1, Mutability::Not);
        let ref3 = c.mk_ref(Region::Erased, ref2, Mutability::Mut);
        (ref3, bool_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: triple_ref_bool,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Deref,
            ProjectionElem::Deref,
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_large_tuple_field_access() {
    let (ctx, (tuple_ty, f3_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let f3_ty = c.mk_ty(TyKind::Uint(UintTy::U64));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let bool_ty = c.bool_ty();
        let substs = c.intern_substitution(vec![
            GenericArg::Ty(i32_ty),
            GenericArg::Ty(f3_ty),
            GenericArg::Ty(u32_ty),
            GenericArg::Ty(bool_ty),
        ]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        (tuple_ty, f3_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_usize(1))]),
    };

    assert_eq!(place.ty(&ctx, &locals), f3_ty);
}
