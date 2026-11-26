use glyim_core::primitives::IntTy;

use super::helpers::{test_frozen_ty_ctx, test_ty_ctx, with_fresh_ty_ctx};
use crate::*;

// S02-T02: freeze() verifies sentinels — the context is constructed
// with sentinel assertions in TyCtxMut::new(). We test that freeze
// succeeds and preserves all data.

#[test]
fn freeze_preserves_sentinels() {
    let ctx = test_frozen_ty_ctx();
    assert!(matches!(ctx.ty_kind(Ty::ERROR), TyKind::Error));
    assert!(matches!(ctx.ty_kind(Ty::NEVER), TyKind::Never));
    assert!(matches!(ctx.ty_kind(Ty::UNIT), TyKind::Unit));
    assert!(matches!(ctx.ty_kind(Ty::BOOL), TyKind::Bool));
}

#[test]
fn freeze_preserves_custom_types() {
    let (frozen, custom_ty) = with_fresh_ty_ctx(|c| c.mk_ty(TyKind::Int(IntTy::I32)));
    assert!(matches!(frozen.ty_kind(custom_ty), TyKind::Int(IntTy::I32)));
}

#[test]
fn freeze_preserves_substitution_data() {
    let (frozen, adt_ty) = with_fresh_ty_ctx(|c| {
        let args = vec![
            GenericArg::Ty(c.bool_ty()),
            GenericArg::Ty(c.mk_ty(TyKind::Int(IntTy::I32))),
        ];
        let substs = c.intern_substitution(args);
        c.mk_adt(glyim_core::def_id::AdtId::from_usize(1), substs)
    });
    if let TyKind::Adt(_, substs) = frozen.ty_kind(adt_ty) {
        let args = frozen.substitution_args(*substs);
        assert_eq!(args.len(), 2);
    } else {
        panic!("expected Adt");
    }
}

#[test]
fn frozen_context_has_sentinel_accessors() {
    let ctx = test_frozen_ty_ctx();
    assert_eq!(ctx.error_ty(), Ty::ERROR);
    assert_eq!(ctx.never_ty(), Ty::NEVER);
    assert_eq!(ctx.unit_ty(), Ty::UNIT);
    assert_eq!(ctx.bool_ty(), Ty::BOOL);
}

#[test]
fn freeze_preserves_type_flags() {
    let (frozen, bool_ty) = with_fresh_ty_ctx(|c| c.bool_ty());
    let flags = frozen.ty_flags(bool_ty);
    assert!(!flags.contains(TypeFlags::HAS_TY_INFER));
    assert!(!flags.contains(TypeFlags::HAS_ERROR));
}

#[test]
fn freeze_allows_region_access() {
    let mut ctx = test_ty_ctx();
    let vid = ctx.new_region_var(Region::Erased);
    let frozen = ctx.freeze();
    let region = frozen.region(vid);
    assert!(matches!(region, Region::Erased));
}
