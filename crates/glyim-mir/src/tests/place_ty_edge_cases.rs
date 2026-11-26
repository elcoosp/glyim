use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Const, ConstKind, FieldIdx, GenericArg, Region, Ty, TyCtxMut, TyKind};

#[test]
fn ty_empty_projection_on_error_local() {
    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
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
fn ty_empty_projection_on_unit_local() {
    let (ctx, unit_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.unit_ty());

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: unit_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place::new(local);
    assert_eq!(place.ty(&ctx, &locals), Ty::UNIT);
}

#[test]
fn ty_deref_on_string_type_returns_error() {
    let (ctx, string_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::String));

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: string_ty,
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
fn ty_field_on_bool_returns_error() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
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
fn ty_field_on_int_returns_error() {
    let (ctx, i32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Int(IntTy::I32)));

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
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
fn ty_index_on_bool_returns_error() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
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
fn ty_index_on_ref_to_array() {
    let (ctx, (ref_array_ty, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(10),
            ty: usize_ty,
        };
        let array_ty = c.mk_ty(TyKind::Array(bool_ty, len));
        let ref_array_ty = c.mk_ref(Region::Erased, array_ty, Mutability::Not);
        (ref_array_ty, bool_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_array_ty,
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
    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}

#[test]
fn ty_downcast_then_field_on_tuple() {
    let (ctx, (tuple_ty, u32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        (tuple_ty, u32_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Field(FieldIdx::from_usize(1)),
        ]),
    };
    assert_eq!(place.ty(&ctx, &locals), u32_ty);
}

#[test]
fn ty_multiple_downcasts_idempotent() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        c.mk_ty(TyKind::Tuple(substs))
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Downcast(VariantIdx::from_usize(0)),
            ProjectionElem::Downcast(VariantIdx::from_usize(1)),
            ProjectionElem::Downcast(VariantIdx::from_usize(2)),
        ]),
    };
    assert_eq!(place.ty(&ctx, &locals), tuple_ty);
}

#[test]
fn ty_deref_on_slice_returns_error() {
    let (ctx, slice_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ty(TyKind::Slice(i32_ty))
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: slice_ty,
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
fn ty_deref_on_array_returns_error() {
    let (ctx, array_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(5),
            ty: usize_ty,
        };
        c.mk_ty(TyKind::Array(bool_ty, len))
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: array_ty,
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
fn ty_chained_deref_field_index() {
    let (ctx, (ref_tuple_slice_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = c.mk_ty(TyKind::Slice(i32_ty));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(slice_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        let ref_tuple = c.mk_ref(Region::Erased, tuple_ty, Mutability::Not);
        (ref_tuple, i32_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_tuple_slice_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_usize(0)),
            ProjectionElem::Index(LocalIdx::from_usize(1)),
        ]),
    };
    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_raw_ptr_to_ref_to_inner() {
    let (ctx, (raw_ptr_ref_bool, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let ref_bool = c.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let raw_ptr = c.mk_ty(TyKind::RawPtr(ref_bool, Mutability::Not));
        (raw_ptr, bool_ty)
    });

    let local = LocalIdx::from_usize(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: raw_ptr_ref_bool,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref, ProjectionElem::Deref]),
    };
    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}
