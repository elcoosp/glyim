//! Tests for Predicate, `TraitPredicate`, `TraitRef`, `ImplPolarity`.

use super::helpers::with_fresh_ty_ctx;
use crate::*;

#[test]
fn trait_predicate_positive() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let trait_ref = TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(1),
            substs,
        };
        let pred = TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Positive,
        };
        assert!(matches!(pred.polarity, ImplPolarity::Positive));
    });
    let _ = ctx;
}

#[test]
fn trait_predicate_negative() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        let trait_ref = TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(2),
            substs,
        };
        let pred = TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Negative,
        };
        assert!(matches!(pred.polarity, ImplPolarity::Negative));
    });
    let _ = ctx;
}

#[test]
fn predicate_trait_variant() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![]);
        let trait_ref = TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(1),
            substs,
        };
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Positive,
        });
        assert!(matches!(pred, Predicate::Trait(_)));
    });
    let _ = ctx;
}

#[test]
fn predicate_region_outlives() {
    let pred = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: Region::Static,
        b: Region::Erased,
    });
    assert!(matches!(pred, Predicate::RegionOutlives(_)));
}

#[test]
fn predicate_type_outlives() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let pred = Predicate::TypeOutlives(TypeOutlivesPredicate {
            ty: c.bool_ty(),
            region: Region::Static,
        });
        assert!(matches!(pred, Predicate::TypeOutlives(_)));
    });
    let _ = ctx;
}

#[test]
fn predicate_well_formed() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let pred = Predicate::WellFormed(c.bool_ty());
        assert!(matches!(pred, Predicate::WellFormed(_)));
    });
    let _ = ctx;
}

#[test]
fn predicate_coerce() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let pred = Predicate::Coerce(
            c.bool_ty(),
            c.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32)),
        );
        assert!(matches!(pred, Predicate::Coerce(_, _)));
    });
    let _ = ctx;
}

#[test]
fn impl_polarity_copy() {
    let pos = ImplPolarity::Positive;
    let neg = ImplPolarity::Negative;
    assert_eq!(pos, ImplPolarity::Positive);
    assert_eq!(neg, ImplPolarity::Negative);
    assert_ne!(pos, neg);
}

#[test]
fn trait_ref_debug() {
    let (ctx, ()) = with_fresh_ty_ctx(|c| {
        let substs = c.intern_substitution(vec![GenericArg::Ty(c.bool_ty())]);
        let tr = TraitRef {
            def_id: glyim_core::def_id::TraitDefId::from_raw(42),
            substs,
        };
        let debug = format!("{tr:?}");
        assert!(debug.contains("42"));
    });
    let _ = ctx;
}

#[test]
fn predicate_equality() {
    let p1 = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: Region::Static,
        b: Region::Erased,
    });
    let p2 = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: Region::Static,
        b: Region::Erased,
    });
    assert_eq!(p1, p2);
}
