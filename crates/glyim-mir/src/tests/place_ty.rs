use crate::*;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_span::Span;
use glyim_test::with_fresh_ty_ctx;
use glyim_type::{Const, ConstKind, FieldIdx, GenericArg, Region, Ty, TyCtxMut, TyKind};

#[test]
fn place_new_creates_empty_projection() {
    let local = LocalIdx::from_raw(0);
    let place = Place::new(local);
    assert_eq!(place.local, local);
    assert!(place.projection.is_empty());
}

#[test]
fn ty_deref_on_ref_returns_inner() {
    let (ctx, ref_bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Not)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_deref_on_mut_ref_returns_inner() {
    let (ctx, ref_mut_bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.bool_ty();
        c.mk_ref(Region::Erased, inner, Mutability::Mut)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_mut_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_deref_on_raw_const_ptr_returns_inner() {
    let (ctx, raw_ptr_bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::RawPtr(inner, Mutability::Not))
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: raw_ptr_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_deref_on_raw_mut_ptr_returns_inner() {
    let (ctx, raw_mut_ptr_bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let inner = c.bool_ty();
        c.mk_ty(TyKind::RawPtr(inner, Mutability::Mut))
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: raw_mut_ptr_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_deref_on_non_pointer_returns_error() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
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
fn ty_field_on_tuple_returns_correct_arg() {
    let (ctx, (tuple_ty, i32_ty, _u32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        (tuple_ty, i32_ty, u32_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_subslice_on_slice_returns_slice_type() {
    // Slicing a slice must yield a slice type `[T]`, not the element type `T`.
    // Regression guard for the Subslice projection in `Place::ty`.
    let (ctx, slice_i32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let elem = c.mk_ty(TyKind::Int(IntTy::I32));
        c.mk_ty(TyKind::Slice(elem))
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: slice_i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Subslice {
            from: 1,
            to: 2,
            from_end: false,
        }]),
    };

    // The subslice of a slice is itself a slice (same type), not the element type.
    assert_eq!(place.ty(&ctx, &locals), slice_i32_ty);
}

#[test]
fn ty_mut_subslice_on_array_returns_array_type() {
    // Subslice of a fixed-size array `[T; N]` must yield `[T; len]`, not the
    // element type `T` and not the full `[T; N]`. Regression test for plan §11.1.
    let (ctx, (expected, got)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let elem = c.mk_ty(TyKind::Int(IntTy::I32));
        let arr5 = c.mk_ty(TyKind::Array(
            elem,
            Const {
                kind: ConstKind::Uint(5),
                ty: Ty::USIZE,
            },
        ));
        // `arr[1..3]` has length 3 - 1 = 2.
        let expected = c.mk_ty(TyKind::Array(
            elem,
            Const {
                kind: ConstKind::Uint(2),
                ty: Ty::USIZE,
            },
        ));

        let local = LocalIdx::from_raw(0);
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: arr5,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        let place = Place {
            local,
            projection: Box::new([ProjectionElem::Subslice {
                from: 1,
                to: 3,
                from_end: false,
            }]),
        };
        let got = place.ty_mut(c, &locals);
        (expected, got)
    });
    let _ = ctx;
    assert_eq!(
        ctx.ty_kind(expected),
        ctx.ty_kind(got),
        "subslice [1..3] of [i32; 5] should be [i32; 2]"
    );
}

#[test]
fn ty_mut_subslice_from_end_on_array_returns_array_type() {
    // `arr[1..^2]` (from_end) drops 2 trailing elements: end index = 5 - 2 = 3,
    // length = 3 - 1 = 2, so the result is `[i32; 2]`.
    let (ctx, (expected, got)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let elem = c.mk_ty(TyKind::Int(IntTy::I32));
        let arr5 = c.mk_ty(TyKind::Array(
            elem,
            Const {
                kind: ConstKind::Uint(5),
                ty: Ty::USIZE,
            },
        ));
        let expected = c.mk_ty(TyKind::Array(
            elem,
            Const {
                kind: ConstKind::Uint(2),
                ty: Ty::USIZE,
            },
        ));

        let local = LocalIdx::from_raw(0);
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: arr5,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        let place = Place {
            local,
            projection: Box::new([ProjectionElem::Subslice {
                from: 1,
                to: 2,
                from_end: true,
            }]),
        };
        let got = place.ty_mut(c, &locals);
        (expected, got)
    });
    let _ = ctx;
    assert_eq!(
        ctx.ty_kind(expected),
        ctx.ty_kind(got),
        "subslice [1..^2] of [i32; 5] should be [i32; 2]"
    );
}

#[test]
fn ty_mut_subslice_on_slice_still_returns_slice_type() {
    // Consistency: a subslice of a slice base via `ty_mut` returns the same
    // slice type (no new allocation needed).
    let (ctx, (expected, got)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let elem = c.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = c.mk_ty(TyKind::Slice(elem));

        let local = LocalIdx::from_raw(0);
        let mut locals = IndexVec::new();
        locals.push(LocalDecl {
            ty: slice_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });

        let place = Place {
            local,
            projection: Box::new([ProjectionElem::Subslice {
                from: 1,
                to: 3,
                from_end: false,
            }]),
        };
        let got = place.ty_mut(c, &locals);
        (slice_ty, got)
    });
    let _ = ctx;
    assert_eq!(
        ctx.ty_kind(expected),
        ctx.ty_kind(got),
        "subslice of a slice must stay the same slice type"
    );
}

#[test]
fn ty_field_on_tuple_second_element() {
    let (ctx, (tuple_ty, _i32_ty, u32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        (tuple_ty, i32_ty, u32_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(1))]),
    };

    assert_eq!(place.ty(&ctx, &locals), u32_ty);
}

#[test]
fn ty_field_on_tuple_out_of_bounds_returns_error() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        c.mk_ty(TyKind::Tuple(substs))
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(5))]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_field_on_non_tuple_returns_error() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Field(FieldIdx::from_raw(0))]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_index_on_array_returns_element() {
    let (ctx, (array_ty, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(5),
            ty: usize_ty,
        };
        let array_ty = c.mk_ty(TyKind::Array(bool_ty, len));
        (array_ty, bool_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let index_local = LocalIdx::from_raw(1);
    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(index_local)]),
    };

    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}

#[test]
fn ty_index_on_slice_returns_element() {
    let (ctx, (slice_ty, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let slice_ty = c.mk_ty(TyKind::Slice(bool_ty));
        (slice_ty, bool_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: slice_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let index_local = LocalIdx::from_raw(1);
    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(index_local)]),
    };

    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}

#[test]
fn ty_index_on_non_array_slice_returns_error() {
    let (ctx, bool_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let index_local = LocalIdx::from_raw(1);
    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Index(index_local)]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_downcast_returns_same_type() {
    let (ctx, tuple_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        c.mk_ty(TyKind::Tuple(substs))
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Downcast(VariantIdx::from_raw(0))]),
    };

    assert_eq!(place.ty(&ctx, &locals), tuple_ty);
}

#[test]
fn ty_chained_projections() {
    let (ctx, (ref_tuple_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(i32_ty), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        let ref_tuple_ty = c.mk_ref(Region::Erased, tuple_ty, Mutability::Not);
        (ref_tuple_ty, i32_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_tuple_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_double_deref() {
    let (ctx, (ref_ref_bool_ty, _bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let ref_bool_ty = c.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let ref_ref_bool_ty = c.mk_ref(Region::Erased, ref_bool_ty, Mutability::Not);
        (ref_ref_bool_ty, bool_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_ref_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref, ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::BOOL);
}

#[test]
fn ty_deref_on_mut_ref_to_array_then_index() {
    let (ctx, (ref_mut_array_ty, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let usize_ty = c.mk_ty(TyKind::Uint(UintTy::Usize));
        let len = Const {
            kind: ConstKind::Uint(3),
            ty: usize_ty,
        };
        let array_ty = c.mk_ty(TyKind::Array(bool_ty, len));
        let ref_mut_array_ty = c.mk_ref(Region::Erased, array_ty, Mutability::Mut);
        (ref_mut_array_ty, bool_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_mut_array_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Index(LocalIdx::from_raw(1)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}

#[test]
fn ty_no_projection_returns_local_type() {
    let (ctx, i32_ty) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.mk_ty(TyKind::Int(IntTy::I32)));

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place::new(local);
    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}

#[test]
fn ty_triple_chain_deref_field_deref() {
    let (ctx, (ref_tuple_ref_bool_ty, bool_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let bool_ty = c.bool_ty();
        let ref_bool = c.mk_ref(Region::Erased, bool_ty, Mutability::Not);
        let u32_ty = c.mk_ty(TyKind::Uint(UintTy::U32));
        let substs = c.intern_substitution(vec![GenericArg::Ty(ref_bool), GenericArg::Ty(u32_ty)]);
        let tuple_ty = c.mk_ty(TyKind::Tuple(substs));
        let ref_tuple = c.mk_ref(Region::Erased, tuple_ty, Mutability::Not);
        (ref_tuple, bool_ty)
    });

    let local = LocalIdx::from_raw(0);
    let mut locals = IndexVec::new();
    locals.push(LocalDecl {
        ty: ref_tuple_ref_bool_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let place = Place {
        local,
        projection: Box::new([
            ProjectionElem::Deref,
            ProjectionElem::Field(FieldIdx::from_raw(0)),
            ProjectionElem::Deref,
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), bool_ty);
}

#[test]
fn ty_deref_on_never_type_returns_error() {
    let local = LocalIdx::from_raw(0);
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::NEVER,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let (ctx, _) = with_fresh_ty_ctx(|c: &mut TyCtxMut| c.bool_ty());

    let place = Place {
        local,
        projection: Box::new([ProjectionElem::Deref]),
    };

    assert_eq!(place.ty(&ctx, &locals), Ty::ERROR);
}

#[test]
fn ty_index_on_ref_to_slice() {
    let (ctx, (ref_slice_ty, i32_ty)) = with_fresh_ty_ctx(|c: &mut TyCtxMut| {
        let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
        let slice_ty = c.mk_ty(TyKind::Slice(i32_ty));
        let ref_slice_ty = c.mk_ref(Region::Erased, slice_ty, Mutability::Not);
        (ref_slice_ty, i32_ty)
    });

    let local = LocalIdx::from_raw(0);
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
            ProjectionElem::Index(LocalIdx::from_raw(1)),
        ]),
    };

    assert_eq!(place.ty(&ctx, &locals), i32_ty);
}
