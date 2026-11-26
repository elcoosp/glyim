use crate::*;
use glyim_core::primitives::*;
use glyim_type::{InferVar, TyKind};

#[test]
fn test_property_generator_concrete() {
    let mut ctx = test_ty_ctx();
    let mut generator = property::arbitrary::Generator::new(42);
    let ty = generator.generate_ty(&mut ctx, 0);
    let frozen = ctx.freeze();
    assert!(!matches!(frozen.ty_kind(ty), TyKind::Error));
    property::arbitrary::sentinel_invariant(&frozen);
}

#[test]
fn test_property_generator_with_infer() {
    let mut ctx = test_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut generator = property::arbitrary::Generator::new(123);
    let ty = generator.generate_ty_with_infer(&mut ctx, &mut infer, 0);
    let frozen = ctx.freeze();
    let kind = frozen.ty_kind(ty);
    assert!(!matches!(kind, TyKind::Error));
}

#[test]
fn test_property_generator_depth_limit() {
    let mut ctx = test_ty_ctx();
    let mut generator = property::arbitrary::Generator::new(999).with_max_depth(0);
    let ty = generator.generate_ty(&mut ctx, 5);
    let frozen = ctx.freeze();
    assert!(!matches!(frozen.ty_kind(ty), TyKind::Error));
}

#[test]
fn test_unification_var_with_concrete() {
    let mut ctx = test_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let var = infer.new_ty_var(&mut ctx);
    let var_ty = ctx.mk_ty(TyKind::Infer(InferVar::Ty(var)));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    property::unify::test_unify_var_with_concrete(&mut ctx, &mut infer, var_ty, i32_ty);
}

#[test]
fn test_unification_same_type() {
    let mut ctx = test_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    property::unify::test_unify_same_type_succeeds(&mut ctx, &mut infer, i32_ty);
}

#[test]
fn test_unification_different_types_fail() {
    let mut ctx = test_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    property::unify::test_unify_different_types_fails(&mut ctx, &mut infer, i32_ty, bool_ty);
}

#[test]
fn test_check_ty_property() {
    let result = check_ty_property(42, 50, |_ctx, _ty| Ok(()));
    assert!(result.is_ok());
}

#[test]
fn test_check_ty_property_failure() {
    let result = check_ty_property(42, 10, |_ctx, _ty| Err("bad type".to_string()));
    assert!(result.is_err());
}

#[test]
fn property_unify_ty_with_itself_always_succeeds() {
    // M4 oracle (M4.11 naive-unifier direction): unifying a type with itself
    // always succeeds and yields the same type. Run over 128 generated types
    // — past the 100-iteration bar — exercising the generator's full variant
    // range (bool/never/unit/Int/Uint/Float/Ref/Slice at depth). The
    // inference table's `a == b` fast path must short-circuit to `Ok` for
    // every generated `Ty`.
    const N: usize = 128;
    let mut ctx = test_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut generator = property::arbitrary::Generator::new(2026);
    for i in 0..N {
        let ty = generator.generate_ty(&mut ctx, 0);
        let result = infer.unify(&mut ctx, ty, ty, glyim_span::Span::DUMMY);
        assert!(
            result.is_ok(),
            "case {i}: unify(ty, ty) should succeed but failed: {:?}",
            result.err()
        );
    }
}

#[test]
fn property_mir_every_used_local_is_declared() {
    // M4 oracle (M5.5 def-before-use slice): every local a MIR statement
    // references is declared in `body.locals`. The generator produces
    // well-formed bodies by construction, so the declaredness validator must
    // accept all 128 of them; the negative partner proves the validator
    // catches an injected undeclared local rather than rubber-stamping them.
    const N: usize = 128;
    let mut body_gen = property::mir::MirBodyGenerator::new(4242);
    let owner = glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_usize(0),
        glyim_core::def_id::LocalDefId::from_usize(0),
    );
    for i in 0..N {
        let body = body_gen.generate_well_formed_body(owner);
        assert!(
            glyim_mir::validate::check_local_declaredness(&body).is_ok(),
            "case {i}: well-formed body rejected by the declaredness oracle"
        );
    }
    // Negative: an undeclared local must be caught.
    let bad = body_gen.generate_undeclared_local_body(owner);
    assert!(
        glyim_mir::validate::check_local_declaredness(&bad).is_err(),
        "undeclared local was not caught by the declaredness oracle"
    );
}

/// M5.5 / M4 fuzz: throw 10 000 mutated MIR bodies at the unified validator
/// and assert (a) zero panics — the loop completing is the proof, since a
/// panic would fail the test — and (b) every rejection is a typed
/// [`glyim_mir::validate::Violation`] (the return type is `Vec<Violation>`,
/// so an `Err` always carries typed violations by construction).
///
/// [`property::mir::MirBodyGenerator::generate_malformed_body`] injects one
/// of six historical bug classes (audit MIR-7/10/28/30/31 plus storage
/// pairing and cleanup-edge validity) per body. Both validator entry points
/// are exercised: [`glyim_mir::validate::validate_body`] (the always-on
/// invariants) and [`glyim_mir::validate::validate_no_subslice`] (the
/// `slice_desugar` post-condition) — the latter catches the `Subslice` class
/// that the former intentionally accepts pre-`slice_desugar`.
#[test]
fn property_mir_validator_never_panics_on_10k_malformed_bodies() {
    const N: usize = 10_000;
    let ctx = test_ty_ctx();
    let frozen = ctx.freeze();
    let mut body_gen = property::mir::MirBodyGenerator::new(2026);
    let owner = glyim_core::def_id::DefId::new(
        glyim_core::def_id::CrateId::from_usize(0),
        glyim_core::def_id::LocalDefId::from_usize(0),
    );
    let mut rejected = 0usize;
    for _ in 0..N {
        let body = body_gen.generate_malformed_body(owner);
        // Both validator entry points must complete without panicking.
        let r1 = glyim_mir::validate::validate_body(&frozen, &body);
        let r2 = glyim_mir::validate::validate_no_subslice(&body);
        for r in [r1, r2] {
            if let Err(violations) = r {
                // Every rejection is a typed Violation batch (never empty,
                // never a panic).
                assert!(
                    !violations.is_empty(),
                    "a rejection must carry at least one typed Violation"
                );
                rejected += 1;
            }
        }
    }
    // At least one of the 10 000 malformed bodies must be rejected — the
    // sanity guard that the generator actually injected violations.
    assert!(
        rejected > 0,
        "at least some malformed bodies must be rejected by the validator"
    );
}
