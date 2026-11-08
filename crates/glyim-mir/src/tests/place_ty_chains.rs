//! Tests for complex projection chains in `Place::ty()`

use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::IntTy;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, FieldIdx, GenericArg, Region, TyCtxMut, TyKind};

#[test]
fn test_nested_ref_field_access() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let tuple_ty = {
        let subst = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(i32_ty)]);
        c.mk_ty(TyKind::Tuple(subst))
    };
    let ref_ty = c.mk_ref(
        Region::Erased,
        tuple_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ref_ref_ty = c.mk_ref(
        Region::Erased,
        ref_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_ref_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_array_of_refs() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = c.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let const_len = Const {
        kind: ConstKind::Int(10),
        ty: c.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize)),
    };
    let array_ty = c.mk_ty(TyKind::Array(ref_ty, const_len));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Index(LocalIdx::from_raw(1)),
            ProjectionElem::Deref,
        ]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_slice_of_refs() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = c.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let slice_ty = c.mk_ty(TyKind::Slice(ref_ty));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: slice_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Index(LocalIdx::from_raw(1)),
            ProjectionElem::Deref,
        ]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_tuple_of_arrays() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let const_len = Const {
        kind: ConstKind::Int(5),
        ty: c.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize)),
    };
    let array_ty = c.mk_ty(TyKind::Array(i32_ty, const_len));
    let subst = c.intern_substitution(vec![GenericArg::Ty(array_ty), GenericArg::Ty(array_ty)]);
    let tuple_ty = c.mk_ty(TyKind::Tuple(subst));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Field(FieldIdx::from_raw(0)),
            ProjectionElem::Index(LocalIdx::from_raw(1)),
        ]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}
