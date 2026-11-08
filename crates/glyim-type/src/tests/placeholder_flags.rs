//! Tests for `HAS_RE_PLACEHOLDER` and `HAS_TY_PLACEHOLDER` flags

use crate::display::TypeLookup;
use crate::*;
use crate::{
    BoundRegionKind, BoundTy, BoundTyKind, PlaceholderRegion, Region, TyKind, TypeFlags,
    UniverseIndex, compute_flags,
};
use glyim_core::interner::Interner;
use glyim_core::primitives::{IntTy, Mutability};

/// A minimal `TypeLookup` implementation for testing `compute_flags`.
struct TestLookup<'a> {
    ctx: &'a TyCtxMut,
}

impl TypeLookup for TestLookup<'_> {
    fn ty_kind(&self, ty: crate::Ty) -> &TyKind {
        self.ctx.ty_kind(ty)
    }
    fn ty_flags(&self, ty: crate::Ty) -> TypeFlags {
        self.ctx.ty_flags(ty)
    }
    fn substitution_args(&self, sub: crate::Substitution) -> &[crate::GenericArg] {
        self.ctx.substitution_args(sub)
    }
    fn name_str(&self, name: glyim_core::interner::Name) -> &str {
        self.ctx.name_str(name)
    }
    fn error_ty(&self) -> crate::Ty {
        self.ctx.error_ty()
    }
}

#[test]
fn has_re_placeholder_flag_defined() {
    assert!(TypeFlags::HAS_RE_PLACEHOLDER.bits() != 0);
    assert!(TypeFlags::HAS_TY_PLACEHOLDER.bits() != 0);
    assert_ne!(
        TypeFlags::HAS_RE_PLACEHOLDER.bits(),
        TypeFlags::HAS_TY_PLACEHOLDER.bits()
    );
}

#[test]
fn has_re_placeholder_not_set_on_static_region() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(Region::Static, i32_ty, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(ref_ty), &lookup, 0);
    assert!(
        !flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Static region should not set HAS_RE_PLACEHOLDER"
    );
}

#[test]
fn has_re_placeholder_set_on_placeholder_region() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let ref_ty = ctx.mk_ref(Region::Placeholder(placeholder), i32_ty, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(ref_ty), &lookup, 0);
    assert!(
        flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Placeholder region should set HAS_RE_PLACEHOLDER"
    );
}

#[test]
fn has_re_placeholder_not_set_on_erased_region() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(Region::Erased, i32_ty, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(ref_ty), &lookup, 0);
    assert!(
        !flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Erased region should not set HAS_RE_PLACEHOLDER"
    );
}

#[test]
fn has_re_placeholder_not_set_on_var_region() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let region_var_vid = ctx.new_region_var(Region::Erased);
    let region_var = Region::Var(region_var_vid);
    let ref_ty = ctx.mk_ref(region_var, i32_ty, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(ref_ty), &lookup, 0);
    assert!(
        !flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Inference region var should not set HAS_RE_PLACEHOLDER"
    );
    assert!(
        flags.contains(TypeFlags::HAS_RE_INFER),
        "Inference region var should set HAS_RE_INFER"
    );
}

#[test]
fn has_ty_placeholder_set_on_bound_type() {
    let mut ctx = TyCtxMut::new(Interner::new());
    let bound_ty = ctx.mk_ty(TyKind::Bound(
        0,
        BoundTy {
            var: 0,
            kind: BoundTyKind::Anon,
        },
    ));

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(bound_ty), &lookup, 0);
    assert!(
        flags.contains(TypeFlags::HAS_TY_PLACEHOLDER),
        "Bound type should set HAS_TY_PLACEHOLDER"
    );
}

#[test]
fn has_re_placeholder_in_nested_ref() {
    // Test placeholder region in nested reference: &&placeholder i32
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let inner_ref = ctx.mk_ref(Region::Placeholder(placeholder), i32_ty, Mutability::Not);
    let outer_ref = ctx.mk_ref(Region::Static, inner_ref, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(outer_ref), &lookup, 0);
    assert!(
        flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Nested placeholder region should propagate HAS_RE_PLACEHOLDER"
    );
}

#[test]
fn has_re_placeholder_in_tuple() {
    // Test placeholder region in tuple element
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let ref_ty = ctx.mk_ref(Region::Placeholder(placeholder), i32_ty, Mutability::Not);
    let substs = ctx.intern_substitution(vec![crate::GenericArg::Ty(ref_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(substs));

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(tuple_ty), &lookup, 0);
    assert!(
        flags.contains(TypeFlags::HAS_RE_PLACEHOLDER),
        "Placeholder in tuple should propagate HAS_RE_PLACEHOLDER"
    );
}

#[test]
fn placeholder_region_no_other_spurious_flags() {
    // Placeholder region should ONLY set HAS_RE_PLACEHOLDER, not HAS_RE_INFER or HAS_RE_PARAM
    let mut ctx = TyCtxMut::new(Interner::new());
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let placeholder = PlaceholderRegion {
        universe: UniverseIndex(1),
        bound: BoundRegionKind::BrAnon(0),
        index: 0,
    };
    let ref_ty = ctx.mk_ref(Region::Placeholder(placeholder), i32_ty, Mutability::Not);

    let lookup = TestLookup { ctx: &ctx };
    let flags = compute_flags(ctx.ty_kind(ref_ty), &lookup, 0);
    assert!(flags.contains(TypeFlags::HAS_RE_PLACEHOLDER));
    assert!(
        !flags.contains(TypeFlags::HAS_RE_INFER),
        "Placeholder should not set HAS_RE_INFER"
    );
    assert!(
        !flags.contains(TypeFlags::HAS_RE_PARAM),
        "Placeholder should not set HAS_RE_PARAM"
    );
}
