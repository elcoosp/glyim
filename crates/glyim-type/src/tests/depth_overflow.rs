//! Tests for `TypeFlags` `HAS_DEPTH_OVERFLOW`.
//!
//! Note: The current `compute_flags` implementation uses pre-computed flags
//! (`ctx.ty_flags()`) for inner types rather than recursing into `compute_flags`.
//! This means the depth parameter in `compute_flags` only applies to the top-level
//! call, and deep nesting does NOT trigger `HAS_DEPTH_OVERFLOW`. The depth
//! parameter exists for potential future recursive use. These tests verify
//! the actual behavior.

use glyim_core::primitives::Mutability;

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn deep_nesting_does_not_overflow_with_current_impl() {
    // With the current bottom-up flag computation strategy, deeply nested
    // types do NOT trigger HAS_DEPTH_OVERFLOW because each type's flags
    // are computed independently using pre-computed inner flags.
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let mut current = c.bool_ty();
        for _ in 0..70 {
            current = c.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    let flags = frozen.ty_flags(ty);
    // With current implementation, no overflow occurs
    assert!(
        !flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW),
        "current bottom-up flag computation should not trigger overflow"
    );
}

#[test]
fn moderate_depth_no_overflow() {
    let (frozen, ty) = with_fresh_ty_ctx(|c| {
        let mut current = c.bool_ty();
        for _ in 0..10 {
            current = c.mk_ref(Region::Erased, current, Mutability::Not);
        }
        current
    });
    let flags = frozen.ty_flags(ty);
    assert!(
        !flags.contains(TypeFlags::HAS_DEPTH_OVERFLOW),
        "should not have HAS_DEPTH_OVERFLOW for moderate nesting"
    );
}

#[test]
fn ty_has_depth_overflow_method_returns_false_for_normal_types() {
    let (frozen, ty) =
        with_fresh_ty_ctx(|c| c.mk_ref(Region::Erased, c.bool_ty(), Mutability::Not));
    assert!(!frozen.ty_has_depth_overflow(ty));
}

#[test]
fn depth_overflow_flag_exists_and_can_be_checked() {
    // Verify the flag exists and is distinct
    let flag = TypeFlags::HAS_DEPTH_OVERFLOW;
    assert!(!flag.is_empty());
    assert_ne!(flag, TypeFlags::HAS_ERROR);
    assert_ne!(flag, TypeFlags::HAS_TY_INFER);
}

#[test]
fn depth_overflow_flag_can_combine_with_other_flags() {
    let combined = TypeFlags::HAS_DEPTH_OVERFLOW | TypeFlags::HAS_ERROR;
    assert!(combined.contains(TypeFlags::HAS_DEPTH_OVERFLOW));
    assert!(combined.contains(TypeFlags::HAS_ERROR));
}
