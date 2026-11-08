//! Property-based tests for `Place::ty()` functionality

use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::IntTy;
use glyim_span::Span;
use glyim_type::{Const, ConstKind, FieldIdx, GenericArg, Region, Ty, TyCtxMut, TyKind};

#[test]
fn test_place_ty_deref_on_ref() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = c.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Deref]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_deref_on_mut_ref() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = c.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Mut,
    );
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_ty,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Deref]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_deref_on_raw_ptr() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ptr_ty = c.mk_ty(TyKind::RawPtr(
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    ));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ptr_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Deref]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_field_on_tuple() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let bool_ty = c.bool_ty();
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let string_ty = c.mk_ty(TyKind::String);
    let char_ty = c.mk_ty(TyKind::Char);

    let args = vec![
        GenericArg::Ty(bool_ty),
        GenericArg::Ty(i32_ty),
        GenericArg::Ty(string_ty),
        GenericArg::Ty(char_ty),
    ];
    let subst = c.intern_substitution(args);
    let tuple_ty = c.mk_ty(TyKind::Tuple(subst));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place0 = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };
    let ty0 = place0.ty(&ctx, &locals);
    assert_eq!(ty0, bool_ty);

    let place1 = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(1))]),
    };
    let ty1 = place1.ty(&ctx, &locals);
    assert_eq!(ty1, i32_ty);
}

#[test]
fn test_place_ty_index_on_array() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let const_len = Const {
        kind: ConstKind::Int(5),
        ty: c.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize)),
    };
    let array_ty = c.mk_ty(TyKind::Array(i32_ty, const_len));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(1))]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_index_on_slice() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let slice_ty = c.mk_ty(TyKind::Slice(i32_ty));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: slice_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(1))]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_chained_deref_field() {
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
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, i32_ty);
}

#[test]
fn test_place_ty_deref_on_non_pointer_returns_error() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Deref]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, Ty::ERROR);
}

#[test]
fn test_place_ty_field_on_non_tuple_returns_error() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, Ty::ERROR);
}

#[test]
fn test_place_ty_index_on_non_array_returns_error() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let ctx = c.freeze();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local: LocalIdx::from_raw(0),
        projection: Box::new([ProjectionElem::Index(LocalIdx::from_raw(1))]),
    };

    let ty = place.ty(&ctx, &locals);
    assert_eq!(ty, Ty::ERROR);
}
