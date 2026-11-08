//! Tests for Higher-Ranked Trait Bounds (HRTB).
//!
//! V05-T01: Function bound with for<'a> Fn(&'a u32) compiles (run-pass)
//! V05-T02: Higher-ranked region constraint solving (run-pass)
//! V05-T03: Nested HRTB (run-pass)
//! V05-T04: HRTB with associated types (run-pass)
//! V05-T05: Unmet higher-ranked bound produces error (compile-fail)

use crate::hrtb::*;
use crate::infer::InferenceTable;
use crate::solver::{SimpleTraitSolver, SolverResult, TraitContext, TraitDef, TraitSolver};
use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};
use glyim_type::*;

fn fresh_ctx_and_infer() -> (TyCtxMut, InferenceTable) {
    (TyCtxMut::new(Interner::new()), InferenceTable::new())
}

// V05-T01: Function bound with for<'a> Fn(&'a u32)

#[test]
fn v05_t01_fn_bound_for_a_ref_u32() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let u32_ty = ctx.mk_ty(TyKind::Uint(UintTy::U32));
    let ref_u32 = ctx.mk_ref(bound_region, u32_ty, Mutability::Not);
    let bool_ty = ctx.bool_ty();

    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(ref_u32)]);
    let fn_sig = FnSig {
        inputs,
        output: bool_ty,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));

    let binder = Binder::bind(fn_ptr_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 1);
    assert_eq!(inst.universe, UniverseIndex(1));

    match ctx.ty_kind(inst.value) {
        TyKind::FnPtr(sig) => {
            let input_args = ctx.substitution_args(sig.inputs);
            assert_eq!(input_args.len(), 1);
            if let GenericArg::Ty(input_ty) = input_args[0] {
                if let TyKind::Ref(region, inner, Mutability::Not) = ctx.ty_kind(input_ty) {
                    assert!(
                        matches!(region, Region::Placeholder(_)),
                        "Expected placeholder region, got {region:?}"
                    );
                    assert!(matches!(ctx.ty_kind(*inner), TyKind::Uint(UintTy::U32)));
                } else {
                    panic!("Expected Ref type for input");
                }
            }
            assert!(matches!(ctx.ty_kind(sig.output), TyKind::Bool));
        }
        other => panic!("Expected FnPtr type, got {other:?}"),
    }
}

// V05-T02: Higher-ranked region constraint solving

#[test]
fn v05_t02_higher_ranked_region_constraint_solving() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let trait_def_id = TraitDefId::from_raw(42);

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let substs = ctx.intern_substitution(vec![
        GenericArg::Lifetime(bound_region),
        GenericArg::Ty(i32_ty),
    ]);

    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs,
        },
        polarity: ImplPolarity::Positive,
    };

    let predicate = Predicate::Trait(trait_pred);
    let binder = Binder::bind(predicate, bound_vars);

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    let frozen = ctx.freeze();

    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let result = solver.can_prove(&frozen, tp);
        // Corrected: With strict resolution, no matching impls results in DefiniteNo
        assert_eq!(result, SolverResult::DefiniteNo);
    } else {
        panic!("Expected Trait predicate after instantiation");
    }
}

#[test]
fn v05_t02_higher_ranked_with_impl_proven() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let trait_def_id = TraitDefId::from_raw(100);
    let interner = Interner::new();

    let mut trait_ctx = TraitContext::new();
    trait_ctx.register_trait(TraitDef {
        def_id: trait_def_id,
        name: interner.intern("SomeTrait"),
        associated_types: vec![],
        predicates: vec![],
    });

    let impl_substs = ctx.intern_substitution(vec![]);
    trait_ctx.register_impl(crate::solver::ImplDef {
        def_id: glyim_core::def_id::ImplDefId::from_raw(200),
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs: impl_substs,
        },
        predicates: vec![],
    });

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let substs = ctx.intern_substitution(vec![]);
    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs,
        },
        polarity: ImplPolarity::Positive,
    };

    let predicate = Predicate::Trait(trait_pred);
    let binder = Binder::bind(predicate, bound_vars);

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    let frozen = ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let result = solver.can_prove(&frozen, tp);
        assert_eq!(result, SolverResult::Proven);
    } else {
        panic!("Expected Trait predicate");
    }
}

// V05-T03: Nested HRTB

#[test]
fn v05_t03_nested_hrtb_double_binder() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(1)),
    ]);

    let bound_region_a = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let bound_region_b = Region::LateBound(DebruijnIndex::INNERMOST, 1, BoundRegionKind::BrAnon(1));

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let u32_ty = ctx.mk_ty(TyKind::Uint(UintTy::U32));

    let ref_i32 = ctx.mk_ref(bound_region_a, i32_ty, Mutability::Not);
    let ref_u32 = ctx.mk_ref(bound_region_b, u32_ty, Mutability::Not);

    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(ref_i32), GenericArg::Ty(ref_u32)]);
    let fn_sig = FnSig {
        inputs,
        output: ctx.unit_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));

    let binder = Binder::bind(fn_ptr_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 2);
    assert_eq!(inst.universe, UniverseIndex(1));
}

#[test]
fn v05_t03_nested_hrtb_triple_binder() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(1)),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(2)),
    ]);

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let refs: Vec<GenericArg> = (0..3)
        .map(|i| {
            let bound_region =
                Region::LateBound(DebruijnIndex::INNERMOST, i, BoundRegionKind::BrAnon(i));
            GenericArg::Ty(ctx.mk_ref(bound_region, i32_ty, Mutability::Not))
        })
        .collect();

    let inputs = ctx.intern_substitution(refs);
    let fn_sig = FnSig {
        inputs,
        output: ctx.unit_ty(),
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));

    let binder = Binder::bind(fn_ptr_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 3);
    let indices: Vec<u32> = inst.placeholders.iter().map(|p| p.index).collect();
    assert_eq!(indices, vec![0, 1, 2]);
}

// V05-T04: HRTB with associated types

#[test]
fn v05_t04_hrtb_with_associated_types() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let trait_def_id = TraitDefId::from_raw(55);

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let substs = ctx.intern_substitution(vec![
        GenericArg::Lifetime(bound_region),
        GenericArg::Ty(i32_ty),
    ]);

    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs,
        },
        polarity: ImplPolarity::Positive,
    };

    let predicate = Predicate::Trait(trait_pred);
    let binder = Binder::bind(predicate, bound_vars);

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let args = ctx.substitution_args(tp.trait_ref.substs);
        if let GenericArg::Lifetime(region) = &args[0] {
            assert!(
                matches!(region, Region::Placeholder(_)),
                "Bound region should be replaced with placeholder"
            );
        }
    } else {
        panic!("Expected Trait predicate");
    }

    let frozen = ctx.freeze();
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let result = solver.can_prove(&frozen, tp);
        // Corrected: With strict resolution, no matching impls results in DefiniteNo
        assert_eq!(result, SolverResult::DefiniteNo);
    }
}

// V05-T05: Unmet higher-ranked bound produces error

#[test]
fn v05_t05_unmet_higher_ranked_bound_definite_no() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let trait_def_id = TraitDefId::from_raw(999);

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let substs = ctx.intern_substitution(vec![]);
    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs,
        },
        polarity: ImplPolarity::Positive,
    };

    let predicate = Predicate::Trait(trait_pred);
    let binder = Binder::bind(predicate, bound_vars);

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    let frozen = ctx.freeze();
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let result = solver.can_prove(&frozen, tp);
        // Corrected: With strict resolution, no matching impls results in DefiniteNo
        assert_eq!(result, SolverResult::DefiniteNo);
    }
}

#[test]
fn v05_t05_unmet_bound_negative_polarity() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let trait_def_id = TraitDefId::from_raw(888);

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let substs = ctx.intern_substitution(vec![]);
    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_def_id,
            substs,
        },
        polarity: ImplPolarity::Negative,
    };

    let predicate = Predicate::Trait(trait_pred);
    let binder = Binder::bind(predicate, bound_vars);

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    let frozen = ctx.freeze();
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    if let Predicate::Trait(tp) = &inst.value {
        let result = solver.can_prove(&frozen, tp);
        // Negative polarity with no positive impls evaluates to Ambiguous
        assert_eq!(result, SolverResult::Ambiguous);
    }
}

// Additional substitution tests

#[test]
fn test_substitute_region_outlives_predicate() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(1)),
    ]);

    let region_a = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let region_b = Region::LateBound(DebruijnIndex::INNERMOST, 1, BoundRegionKind::BrAnon(1));

    let predicate = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: region_a,
        b: region_b,
    });
    let binder = Binder::bind(predicate, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    if let Predicate::RegionOutlives(rp) = &inst.value {
        assert!(matches!(rp.a, Region::Placeholder(_)));
        assert!(matches!(rp.b, Region::Placeholder(_)));
    } else {
        panic!("Expected RegionOutlives predicate");
    }
}

#[test]
fn test_substitute_type_outlives_predicate() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let predicate = Predicate::TypeOutlives(TypeOutlivesPredicate {
        ty: i32_ty,
        region: bound_region,
    });
    let binder = Binder::bind(predicate, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    if let Predicate::TypeOutlives(tp) = &inst.value {
        assert!(matches!(ctx.ty_kind(tp.ty), TyKind::Int(IntTy::I32)));
        assert!(matches!(tp.region, Region::Placeholder(_)));
    } else {
        panic!("Expected TypeOutlives predicate");
    }
}

#[test]
fn test_substitute_wellformed_predicate() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);

    let predicate = Predicate::WellFormed(ref_i32);
    let binder = Binder::bind(predicate, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    if let Predicate::WellFormed(ty) = &inst.value {
        if let TyKind::Ref(region, inner, _) = ctx.ty_kind(*ty) {
            assert!(matches!(region, Region::Placeholder(_)));
            assert!(matches!(ctx.ty_kind(*inner), TyKind::Int(IntTy::I32)));
        }
    } else {
        panic!("Expected WellFormed predicate");
    }
}

#[test]
fn test_substitute_coerce_predicate() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);

    let predicate = Predicate::Coerce(ref_i32, i32_ty);
    let binder = Binder::bind(predicate, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    if let Predicate::Coerce(a, b) = &inst.value {
        if let TyKind::Ref(region, _, _) = ctx.ty_kind(*a) {
            assert!(matches!(region, Region::Placeholder(_)));
        }
        assert!(matches!(ctx.ty_kind(*b), TyKind::Int(IntTy::I32)));
    } else {
        panic!("Expected Coerce predicate");
    }
}

#[test]
fn test_universe_increments_with_each_instantiation() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let binder = Binder::bind(
        Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0)),
        bound_vars,
    );

    let inst1 = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    assert_eq!(inst1.universe, UniverseIndex(1));

    let inst2 = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    assert_eq!(inst2.universe, UniverseIndex(2));

    let inst3 = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);
    assert_eq!(inst3.universe, UniverseIndex(3));
}

#[test]
fn test_empty_binder_no_placeholders() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> = Box::new([]);
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let binder = Binder::bind(i32_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 0);
    assert_eq!(inst.universe, UniverseIndex(1));
    assert!(matches!(ctx.ty_kind(inst.value), TyKind::Int(IntTy::I32)));
}

#[test]
fn test_mixed_bound_vars_only_regions_become_placeholders() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Ty(BoundTyKind::Anon),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Const,
    ]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);

    let binder = Binder::bind(ref_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 1);
}

#[test]
fn test_bound_var_substitution_empty() {
    let sub = BoundVarSubstitution::empty();
    assert!(sub.region_map.is_empty());
    assert!(!sub.has_placeholders);
}

#[test]
fn test_hrtb_region_outlives_via_check() {
    let (_ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let region_a = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let predicate = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: region_a.clone(),
        b: region_a,
    });

    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let empty_ctx = TyCtxMut::new(Interner::new());
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, empty_ctx);
    assert_eq!(result, SolverResult::Proven);
}

#[test]
fn test_hrtb_type_outlives_via_check() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let predicate = Predicate::TypeOutlives(TypeOutlivesPredicate {
        ty: i32_ty,
        region: bound_region,
    });
    let binder = Binder::bind(predicate, bound_vars);

    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    assert_eq!(result, SolverResult::Proven);
}

#[test]
fn test_hrtb_coerce_via_check() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let predicate = Predicate::Coerce(i32_ty, i32_ty);
    let binder = Binder::bind(predicate, bound_vars);

    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    // Identity coercion `Coerce(i32, i32)` is provable (a == b), so the cheap
    // HRTB win resolves it to `Proven` instead of the old `Ambiguous`.
    assert_eq!(result, SolverResult::Proven);
}

#[test]
fn test_placeholder_preserves_named_bound_region() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let interner = Interner::new();
    let a_name = interner.intern("a");

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrNamed(a_name))]);

    let binder = Binder::bind(
        Region::LateBound(
            DebruijnIndex::INNERMOST,
            0,
            BoundRegionKind::BrNamed(a_name),
        ),
        bound_vars,
    );

    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    assert_eq!(inst.placeholders.len(), 1);
    assert!(
        matches!(inst.placeholders[0].bound, BoundRegionKind::BrNamed(_)),
        "Placeholder should preserve the named bound region"
    );
}

#[test]
fn test_substitute_fn_ptr_shared_bound_region() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();

    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);

    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let ref_i32_a = ctx.mk_ref(bound_region.clone(), i32_ty, Mutability::Not);
    let ref_i32_b = ctx.mk_ref(bound_region.clone(), i32_ty, Mutability::Not);
    let ref_i32_out = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);

    let inputs =
        ctx.intern_substitution(vec![GenericArg::Ty(ref_i32_a), GenericArg::Ty(ref_i32_b)]);
    let fn_sig = FnSig {
        inputs,
        output: ref_i32_out,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));

    let binder = Binder::bind(fn_ptr_ty, bound_vars);
    let inst = instantiate_binder_with_placeholders(&binder, &mut infer, &mut ctx);

    match ctx.ty_kind(inst.value) {
        TyKind::FnPtr(sig) => {
            let args = ctx.substitution_args(sig.inputs);
            let r0 = if let GenericArg::Ty(t) = args[0] {
                if let TyKind::Ref(r, _, _) = ctx.ty_kind(t) {
                    r.clone()
                } else {
                    panic!("expected ref")
                }
            } else {
                panic!("expected ty")
            };
            let r1 = if let GenericArg::Ty(t) = args[1] {
                if let TyKind::Ref(r, _, _) = ctx.ty_kind(t) {
                    r.clone()
                } else {
                    panic!("expected ref")
                }
            } else {
                panic!("expected ty")
            };
            let r_out = if let TyKind::Ref(r, _, _) = ctx.ty_kind(sig.output) {
                r.clone()
            } else {
                panic!("expected ref")
            };
            assert_eq!(r0, r1, "Both input regions should be the same placeholder");
            assert_eq!(r0, r_out, "Input and output regions should match");
        }
        other => panic!("Expected FnPtr, got {other:?}"),
    }
}

// --- Tier 2.2 cheap-HRTB-win regression tests ---

/// `for<'a> 'a: 'a` must prove (reflexivity).
#[test]
fn t22_region_outlives_reflexive() {
    let (_ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);
    let region_a = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let predicate = Predicate::RegionOutlives(RegionOutlivesPredicate {
        a: region_a.clone(),
        b: region_a,
    });
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let empty_ctx = TyCtxMut::new(Interner::new());
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, empty_ctx);
    assert_eq!(result, SolverResult::Proven);
}

/// `for<'a> 'a: 'static` must prove (static outlives everything), but two
/// *distinct* placeholders must stay Ambiguous (not falsely proven).
#[test]
fn t22_distinct_placeholders_region_outlives_ambiguous() {
    let (_ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> = Box::new([
        BoundVariableKind::Region(BoundRegionKind::BrAnon(0)),
        BoundVariableKind::Region(BoundRegionKind::BrAnon(1)),
    ]);
    let ra = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let rb = Region::LateBound(DebruijnIndex::INNERMOST, 1, BoundRegionKind::BrAnon(1));
    let predicate = Predicate::RegionOutlives(RegionOutlivesPredicate { a: ra, b: rb });
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let empty_ctx = TyCtxMut::new(Interner::new());
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, empty_ctx);
    assert_eq!(result, SolverResult::Ambiguous);
}

/// `for<'a> &'a i32: 'a` must prove (reflexive containment of the bound
/// region inside `T`).
#[test]
fn t22_type_outlives_reflexive() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);
    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx.mk_ref(bound_region.clone(), i32_ty, Mutability::Not);
    let predicate = Predicate::TypeOutlives(TypeOutlivesPredicate {
        ty: ref_i32,
        region: bound_region,
    });
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    assert_eq!(result, SolverResult::Proven);
}

/// `for<'a> i32: 'a` must prove (owned/scalar type, no open components).
#[test]
fn t22_type_outlives_owned() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);
    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let predicate = Predicate::TypeOutlives(TypeOutlivesPredicate {
        ty: i32_ty,
        region: bound_region,
    });
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    assert_eq!(result, SolverResult::Proven);
}

/// `for<'a> &'a i32` well-formedness must prove (concrete, no open components).
#[test]
fn t22_well_formed_concrete() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);
    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);
    let predicate = Predicate::WellFormed(ref_i32);
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    assert_eq!(result, SolverResult::Proven);
}

/// `for<'a> fn(&'a i32) -> fn(&'a i32)` identity coercion must prove.
#[test]
fn t22_coerce_identity_fn_ptr() {
    let (mut ctx, mut infer) = fresh_ctx_and_infer();
    let bound_vars: Box<[BoundVariableKind]> =
        Box::new([BoundVariableKind::Region(BoundRegionKind::BrAnon(0))]);
    let bound_region = Region::LateBound(DebruijnIndex::INNERMOST, 0, BoundRegionKind::BrAnon(0));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let ref_i32 = ctx.mk_ref(bound_region, i32_ty, Mutability::Not);
    let inputs = ctx.intern_substitution(vec![GenericArg::Ty(ref_i32)]);
    let fn_ptr = ctx.mk_ty(TyKind::FnPtr(FnSig {
        inputs,
        output: ref_i32,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    }));
    let predicate = Predicate::Coerce(fn_ptr, fn_ptr);
    let binder = Binder::bind(predicate, bound_vars);
    let trait_ctx = TraitContext::new();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let (result, _frozen) = check_hrtb(&binder, &mut solver, &mut infer, ctx);
    assert_eq!(result, SolverResult::Proven);
}
