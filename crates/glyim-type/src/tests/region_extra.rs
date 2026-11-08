//! Additional region tests - `DebruijnIndex`, `EarlyBound`, `LateBound`, etc.

use super::helpers::{test_ty_ctx, with_fresh_ty_ctx};
use crate::*;

#[test]
fn debruijn_innermost_is_zero() {
    assert_eq!(DebruijnIndex::INNERMOST.0, 0);
}

#[test]
fn debruijn_shifted_in() {
    let idx = DebruijnIndex::INNERMOST;
    let shifted = idx.shifted_in();
    assert_eq!(shifted.0, 1);
    let shifted2 = shifted.shifted_in();
    assert_eq!(shifted2.0, 2);
}

#[test]
fn debruijn_shifted_out() {
    let idx = DebruijnIndex(2);
    let shifted = idx.shifted_out().unwrap();
    assert_eq!(shifted.0, 1);
    let shifted2 = shifted.shifted_out().unwrap();
    assert_eq!(shifted2.0, 0);
    assert!(DebruijnIndex::INNERMOST.shifted_out().is_none());
}

#[test]
fn late_bound_region() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let bound = BoundRegionKind::BrAnon(0);
        let region = Region::LateBound(DebruijnIndex::INNERMOST, 0, bound);
        c.mk_ref(region, c.bool_ty(), glyim_core::primitives::Mutability::Not)
    });
    if let TyKind::Ref(region, _, _) = frozen.ty_kind(ty) {
        match region {
            Region::LateBound(idx, idx2, kind) => {
                assert_eq!(idx.0, 0);
                assert_eq!(*idx2, 0);
                assert!(matches!(kind, BoundRegionKind::BrAnon(0)));
            }
            _ => panic!("expected LateBound"),
        }
    } else {
        panic!("expected Ref");
    }
}

#[test]
fn static_region() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        c.mk_ref(
            Region::Static,
            c.bool_ty(),
            glyim_core::primitives::Mutability::Not,
        )
    });
    if let TyKind::Ref(region, _, _) = frozen.ty_kind(ty) {
        assert!(matches!(region, Region::Static));
    } else {
        panic!("expected Ref");
    }
}

#[test]
fn error_region() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        c.mk_ref(
            Region::Error,
            c.bool_ty(),
            glyim_core::primitives::Mutability::Not,
        )
    });
    if let TyKind::Ref(region, _, _) = frozen.ty_kind(ty) {
        assert!(matches!(region, Region::Error));
    } else {
        panic!("expected Ref");
    }
}

#[test]
fn early_bound_region() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let early = EarlyBoundRegion {
            index: 5,
            name: c.resolver().intern("'a"),
        };
        c.mk_ref(
            Region::EarlyBound(early),
            c.bool_ty(),
            glyim_core::primitives::Mutability::Not,
        )
    });
    if let TyKind::Ref(region, _, _) = frozen.ty_kind(ty) {
        match region {
            Region::EarlyBound(ebr) => {
                assert_eq!(ebr.index, 5);
            }
            _ => panic!("expected EarlyBound"),
        }
    } else {
        panic!("expected Ref");
    }
}

#[test]
fn region_var_count_matches_allocations() {
    let mut ctx = test_ty_ctx();
    assert_eq!(ctx.region_var_count(), 0);
    ctx.new_region_var(Region::Erased);
    assert_eq!(ctx.region_var_count(), 1);
    ctx.new_region_var(Region::Static);
    assert_eq!(ctx.region_var_count(), 2);
}
