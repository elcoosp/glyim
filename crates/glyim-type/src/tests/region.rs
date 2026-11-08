use glyim_core::primitives::Mutability;

use super::helpers::{test_ty_ctx, with_fresh_ty_ctx};
use crate::*;

// S02-T14: Region variable allocation and retrieval

#[test]
fn new_region_var_allocates_and_retrieves() {
    let mut ctx = test_ty_ctx();
    let vid = ctx.new_region_var(Region::Erased);
    let region = ctx.region_var(vid);
    assert!(matches!(region, Region::Erased));
}

#[test]
fn multiple_region_vars() {
    let mut ctx = test_ty_ctx();
    let vid0 = ctx.new_region_var(Region::Erased);
    let vid1 = ctx.new_region_var(Region::Static);
    let vid2 = ctx.new_region_var(Region::Error);
    assert_eq!(vid0.to_raw(), 0);
    assert_eq!(vid1.to_raw(), 1);
    assert_eq!(vid2.to_raw(), 2);
    assert!(matches!(ctx.region_var(vid0), Region::Erased));
    assert!(matches!(ctx.region_var(vid1), Region::Static));
    assert!(matches!(ctx.region_var(vid2), Region::Error));
}

#[test]
fn region_vars_preserved_after_freeze() {
    let (frozen, ()) = with_fresh_ty_ctx(|c| {
        let _v0 = c.new_region_var(Region::Erased);
        let _v1 = c.new_region_var(Region::Static);
    });
    assert!(matches!(
        frozen.region(RegionVid::from_raw(0)),
        Region::Erased
    ));
    assert!(matches!(
        frozen.region(RegionVid::from_raw(1)),
        Region::Static
    ));
}

#[test]
fn ref_with_region_var_has_re_infer_flag() {
    let (frozen, ref_ty) = with_fresh_ty_ctx(|c| {
        let vid = c.new_region_var(Region::Erased);
        let region = Region::Var(vid);
        let inner = c.bool_ty();
        c.mk_ref(region, inner, Mutability::Not)
    });
    let flags = frozen.ty_flags(ref_ty);
    assert!(flags.contains(TypeFlags::HAS_RE_INFER));
}
