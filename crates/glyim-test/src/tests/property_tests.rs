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
        glyim_core::def_id::CrateId::from_raw(0),
        glyim_core::def_id::LocalDefId::from_raw(0),
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
