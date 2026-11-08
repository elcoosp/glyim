//! Property-based tests for `LocalDecl`

use crate::*;
use glyim_core::arena::IndexVec;
use glyim_span::Span;
use glyim_type::{Region, Ty, TyCtxMut, TyKind};

#[test]
fn test_local_decl_creation() {
    let decl = LocalDecl {
        ty: Ty::BOOL,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    assert_eq!(decl.ty, Ty::BOOL);
    assert_eq!(decl.mutability, glyim_core::primitives::Mutability::Not);
}

#[test]
fn test_local_decl_mutable() {
    let decl = LocalDecl {
        ty: Ty::ERROR,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    assert_eq!(decl.mutability, glyim_core::primitives::Mutability::Mut);
    assert!(decl.mutability.is_mut());
}

#[test]
fn test_local_decl_with_custom_span() {
    let span = Span::DUMMY;
    let decl = LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(span),
    };

    assert_eq!(decl.source_info.span, span);
}

#[test]
fn test_local_decl_vector_operations() {
    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();

    let decl1 = LocalDecl {
        ty: Ty::BOOL,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let decl2 = LocalDecl {
        ty: Ty::UNIT,
        mutability: glyim_core::primitives::Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    let idx1: LocalIdx = locals.push(decl1);
    let idx2: LocalIdx = locals.push(decl2);

    assert_eq!(locals[idx1].ty, Ty::BOOL);
    assert_eq!(
        locals[idx1].mutability,
        glyim_core::primitives::Mutability::Not
    );
    assert_eq!(locals[idx2].ty, Ty::UNIT);
    assert_eq!(
        locals[idx2].mutability,
        glyim_core::primitives::Mutability::Mut
    );
    assert_eq!(locals.len(), 2);
}

#[test]
fn test_local_decl_with_complex_type() {
    let mut c = TyCtxMut::new(glyim_core::interner::Interner::new());
    let i32_ty = c.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let ref_ty = c.mk_ref(
        Region::Erased,
        i32_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let ctx = c.freeze();

    let decl = LocalDecl {
        ty: ref_ty,
        mutability: glyim_core::primitives::Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    };

    assert_eq!(decl.ty, ref_ty);
    assert!(matches!(ctx.ty_kind(decl.ty), TyKind::Ref(_, _, _)));
}
