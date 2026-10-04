use crate::{ImplDef, SimpleTraitSolver, SolverResult, TraitContext, TraitDef, TraitSolver};
use glyim_core::Interner;
use glyim_core::def_id::{ImplDefId, TraitDefId};
use glyim_test::{test_ty_ctx, with_fresh_ty_ctx};
use glyim_type::{ImplPolarity, Predicate, Substitution, TraitPredicate, TraitRef, TyCtxMut};

fn empty_subst(ctx: &mut TyCtxMut) -> Substitution {
    ctx.intern_substitution(vec![])
}

#[test]
fn t01_register_trait() {
    let interner = Interner::new();
    let name = interner.intern("MyTrait");
    let mut ctx = TraitContext::new();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(1),
        name,
        associated_types: vec![],
        predicates: vec![],
    });
    assert_eq!(ctx.trait_defs().len(), 1);
    assert_eq!(ctx.trait_defs()[0].name, name);
}

#[test]
fn t02_register_impl() {
    let interner = Interner::new();
    let trait_name = interner.intern("Display");
    let mut ctx = TraitContext::new();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(10),
        name: trait_name,
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ctx_mut = test_ty_ctx();
    let subst = empty_subst(&mut ctx_mut);
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(100),
        trait_ref: TraitRef {
            def_id: TraitDefId::from_raw(10),
            substs: subst,
        },
        predicates: vec![],
    });
    assert_eq!(ctx.impl_defs().len(), 1);
    assert_eq!(
        ctx.impl_defs()[0].trait_ref.def_id,
        TraitDefId::from_raw(10)
    );
}

/// §8.1: builtin/lang traits (`Copy`, `Sized`, `Send`, …) must be discharged
/// structurally by the solver (no user `impl` required). This is the capability
/// the de-stubbing plan flags as missing, which made `T: Copy` / `T: Sized` /
/// `T: Send` bounds barely work.
#[test]
fn t08_builtin_traits_proven_structurally() {
    use crate::BuiltinTrait;
    use glyim_core::primitives::{IntTy, UintTy};
    use glyim_type::{Const, ConstKind, GenericArg, TyKind};

    let mut trait_ctx = TraitContext::new();
    let copy_id = TraitDefId::from_raw(1);
    let sized_id = TraitDefId::from_raw(2);
    let send_id = TraitDefId::from_raw(3);
    trait_ctx.register_lang_trait(copy_id, BuiltinTrait::Copy);
    trait_ctx.register_lang_trait(sized_id, BuiltinTrait::Sized);
    trait_ctx.register_lang_trait(send_id, BuiltinTrait::Send);

    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    // Build every substitution in ONE shared context. `with_fresh_ty_ctx`
    // creates a fresh arena per call, so substitutions minted in separate
    // arenas index into different `substitution_data` stores; proving against
    // a single frozen context then reads out of bounds.
    let mut c = test_ty_ctx();
    let i32_ty = c.mk_ty(TyKind::Int(IntTy::I32));
    let i32_sub = c.intern_substitution(vec![GenericArg::Ty(i32_ty)]);

    let arr_elem = c.mk_ty(TyKind::Int(IntTy::I32));
    let arr_len = Const {
        kind: ConstKind::Uint(3),
        ty: c.mk_ty(TyKind::Uint(UintTy::Usize)),
    };
    let arr_ty = c.mk_ty(TyKind::Array(arr_elem, arr_len));
    let arr_sub = c.intern_substitution(vec![GenericArg::Ty(arr_ty)]);

    let str_ty = c.mk_ty(TyKind::String);
    let str_sub = c.intern_substitution(vec![GenericArg::Ty(str_ty)]);

    let slice_elem = c.mk_ty(TyKind::Int(IntTy::I32));
    let slice_ty = c.mk_ty(TyKind::Slice(slice_elem));
    let slice_sub = c.intern_substitution(vec![GenericArg::Ty(slice_ty)]);

    let ty_ctx = c.freeze();

    let prove = |solver: &mut SimpleTraitSolver, id: TraitDefId, sub: Substitution| -> SolverResult {
        let pred = TraitPredicate {
            trait_ref: TraitRef { def_id: id, substs: sub },
            polarity: ImplPolarity::Positive,
        };
        solver.can_prove(&ty_ctx, &pred)
    };

    // `Copy`
    assert_eq!(prove(&mut solver, copy_id, i32_sub), SolverResult::Proven);
    assert_eq!(prove(&mut solver, copy_id, arr_sub), SolverResult::Proven);
    // `String` is not `Copy` (owning, non-primitive) -> DefiniteNo.
    assert_eq!(prove(&mut solver, copy_id, str_sub), SolverResult::DefiniteNo);

    // `Sized`
    assert_eq!(prove(&mut solver, sized_id, i32_sub), SolverResult::Proven);
    assert_eq!(prove(&mut solver, sized_id, arr_sub), SolverResult::Proven);
    // `[T]` (slice) is unsized -> DefiniteNo.
    assert_eq!(
        prove(&mut solver, sized_id, slice_sub),
        SolverResult::DefiniteNo
    );

    // `Send`
    assert_eq!(prove(&mut solver, send_id, i32_sub), SolverResult::Proven);
    assert_eq!(prove(&mut solver, send_id, arr_sub), SolverResult::Proven);
}

#[test]
fn t03_prove_with_matching_impl() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(2);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Clone"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(200),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.can_prove(&ty_ctx.freeze(), &pred),
        SolverResult::Proven
    );
}

#[test]
fn t04_prove_no_impl_gives_definite_no() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(3);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Default"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.can_prove(&ty_ctx.freeze(), &pred),
        SolverResult::DefiniteNo
    );
}

#[test]
fn t05_impls_of_trait_subset() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let id_a = TraitDefId::from_raw(10);
    let id_b = TraitDefId::from_raw(20);
    ctx.register_trait(TraitDef {
        def_id: id_a,
        name: interner.intern("TraitA"),
        associated_types: vec![],
        predicates: vec![],
    });
    ctx.register_trait(TraitDef {
        def_id: id_b,
        name: interner.intern("TraitB"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(1),
        trait_ref: TraitRef {
            def_id: id_a,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        predicates: vec![],
    });
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(2),
        trait_ref: TraitRef {
            def_id: id_b,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        predicates: vec![],
    });
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(3),
        trait_ref: TraitRef {
            def_id: id_a,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        predicates: vec![],
    });
    let a_impls: Vec<_> = ctx.impls_of_trait(id_a).map(|i| i.def_id).collect();
    assert_eq!(a_impls.len(), 2);
    assert!(a_impls.contains(&ImplDefId::from_raw(1)));
    assert!(a_impls.contains(&ImplDefId::from_raw(3)));
    let b_impls: Vec<_> = ctx.impls_of_trait(id_b).map(|i| i.def_id).collect();
    assert_eq!(b_impls.len(), 1);
    assert_eq!(b_impls[0], ImplDefId::from_raw(2));
}

#[test]
fn t12_evaluate_predicate_trait_delegates_to_can_prove() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(12);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Debug"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(300),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.evaluate_predicate(&ty_ctx.freeze(), &glyim_type::Predicate::Trait(pred)),
        SolverResult::Proven
    );
}

#[test]
fn t13_evaluate_wellformed_outlives_returns_proven() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    trait_ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(13),
        name: interner.intern("Empty"),
        associated_types: vec![],
        predicates: vec![],
    });
    let ty_ctx = test_ty_ctx();
    let bool_ty = ty_ctx.bool_ty();
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    assert_eq!(
        solver.evaluate_predicate(&frozen, &glyim_type::Predicate::WellFormed(bool_ty)),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(
            &frozen,
            &glyim_type::Predicate::TypeOutlives(glyim_type::TypeOutlivesPredicate {
                ty: bool_ty,
                region: glyim_type::Region::Static
            })
        ),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(
            &frozen,
            &glyim_type::Predicate::RegionOutlives(glyim_type::RegionOutlivesPredicate {
                a: glyim_type::Region::Static,
                b: glyim_type::Region::Static
            })
        ),
        SolverResult::Proven
    );
}

#[test]
fn t14_evaluate_coerce_returns_ambiguous() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    trait_ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(14),
        name: interner.intern("Empty"),
        associated_types: vec![],
        predicates: vec![],
    });
    let ty_ctx = test_ty_ctx();
    let bool_ty = ty_ctx.bool_ty();
    let unit_ty = ty_ctx.unit_ty();
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    assert_eq!(
        solver.evaluate_predicate(&frozen, &glyim_type::Predicate::Coerce(bool_ty, unit_ty)),
        SolverResult::Ambiguous
    );
}

#[test]
fn t17_multiple_impls_for_same_trait() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(17);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("From"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(401),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(402),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        predicates: vec![],
    });
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        polarity: ImplPolarity::Positive,
    };
    // Corrected: Multiple matching impls now correctly evaluate to Ambiguous
    assert_eq!(
        solver.can_prove(&ty_ctx.freeze(), &pred),
        SolverResult::Ambiguous
    );
}

#[test]
fn t18_iterator_next_info_uses_real_option_adt() {
    use glyim_core::def_id::FnDefId;
    use glyim_type::{Ty, TyKind};

    let mut trait_ctx = TraitContext::new();
    let solver = SimpleTraitSolver::new(&trait_ctx);

    // Without a registered `next` fn def, resolution yields None.
    let mut ctx = test_ty_ctx();
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    assert!(solver.iterator_next_info(&mut ctx, i32_ty, i32_ty).is_none());

    // With a `next` fn def registered, resolution yields a real info struct
    // whose `option_ty` is the builtin `Option` ADT (id 1010), not a magic id.
    let mut trait_ctx2 = TraitContext::new();
    trait_ctx2.set_builtin_iterator_next(FnDefId::from_raw(42));
    let solver2 = SimpleTraitSolver::new(&trait_ctx2);
    let mut ctx2 = test_ty_ctx();
    let elem_ty = ctx2.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let info = solver2
        .iterator_next_info(&mut ctx2, elem_ty, elem_ty)
        .expect("iterator_next_info should resolve with a registered next fn");
    assert_eq!(info.fn_def_id, FnDefId::from_raw(42));
    match ctx2.freeze().ty_kind(info.option_ty) {
        TyKind::Adt(adt_id, _) => assert_eq!(*adt_id, glyim_core::def_id::AdtId::from_raw(1010)),
        other => panic!("expected Option ADT for option_ty, got {other:?}"),
    }
}

#[test]
fn t18_register_trait_with_associated_types() {
    let interner = Interner::new();
    let assoc = interner.intern("Item");
    let mut ctx = TraitContext::new();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(18),
        name: interner.intern("Iterator"),
        associated_types: vec![assoc],
        predicates: vec![],
    });
    assert_eq!(ctx.trait_defs().len(), 1);
    assert_eq!(ctx.trait_defs()[0].associated_types[0], assoc);
}

#[test]
fn t19_register_impl_with_predicates() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(19);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Default"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(500),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![glyim_type::Predicate::WellFormed(ty_ctx.bool_ty())],
    });
    assert_eq!(trait_ctx.impl_defs().len(), 1);
}

#[test]
fn t20_impls_of_trait_empty_for_no_match() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(20),
        name: interner.intern("NoImpls"),
        associated_types: vec![],
        predicates: vec![],
    });
    assert_eq!(ctx.impls_of_trait(TraitDefId::from_raw(20)).count(), 0);
}

#[test]
fn t22_default_trait_context_is_empty() {
    let ctx = TraitContext::default();
    assert!(ctx.trait_defs().is_empty());
    assert!(ctx.impl_defs().is_empty());
}

#[test]
fn t27_can_prove_negative_impl() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(27);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Send"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(600),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: ty_ctx.intern_substitution(vec![]),
        },
        polarity: ImplPolarity::Negative,
    };
    // Corrected: Negative impl when a positive impl exists evaluates to DefiniteNo
    assert_eq!(
        solver.can_prove(&ty_ctx.freeze(), &pred),
        SolverResult::DefiniteNo
    );
}

#[test]
fn t28_multiple_traits_registered() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    for i in 0..5 {
        ctx.register_trait(TraitDef {
            def_id: TraitDefId::from_raw(100 + i),
            name: interner.intern(&format!("Trait{}", i)),
            associated_types: vec![],
            predicates: vec![],
        });
    }
    assert_eq!(ctx.trait_defs().len(), 5);
}

#[test]
fn t29_register_duplicate_trait_id() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(29),
        name: interner.intern("A"),
        associated_types: vec![],
        predicates: vec![],
    });
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(29),
        name: interner.intern("B"),
        associated_types: vec![],
        predicates: vec![],
    });
    assert_eq!(ctx.trait_defs().len(), 2);
}

#[test]
fn t32_infer_test_helpers() {
    use crate::infer::{InferenceTable, VariableKind};
    use glyim_type::TyKind;
    let mut ctx = test_ty_ctx();
    let mut infer = InferenceTable::new();
    let ty_var = infer.new_ty_var(&mut ctx);
    let int_var = infer.new_int_var(&mut ctx);
    let float_var = infer.new_float_var(&mut ctx);
    assert_eq!(infer.ty_var_kind(ty_var), Some(VariableKind::General));
    let bool_ty = ctx.bool_ty();
    infer.set_ty_var_value(ty_var, bool_ty);
    assert_eq!(infer.probe_ty_var(ty_var), Some(bool_ty));
    let i32_ty = ctx.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
    infer.set_int_var_value(int_var, i32_ty);
    assert_eq!(infer.probe_int_var(int_var), Some(i32_ty));
    let f64_ty = ctx.mk_ty(TyKind::Float(glyim_core::FloatTy::F64));
    infer.set_float_var_value(float_var, f64_ty);
    assert_eq!(infer.probe_float_var(float_var), Some(f64_ty));
}

#[test]
fn t33_register_many_traits_and_impls() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let mut ty_ctx = test_ty_ctx();
    for i in 0..20 {
        let tid = TraitDefId::from_raw(1000 + i);
        ctx.register_trait(TraitDef {
            def_id: tid,
            name: interner.intern(&format!("Trait{}", i)),
            associated_types: vec![],
            predicates: vec![],
        });
        ctx.register_impl(ImplDef {
            def_id: ImplDefId::from_raw(2000 + i),
            trait_ref: TraitRef {
                def_id: tid,
                substs: ty_ctx.intern_substitution(vec![]),
            },
            predicates: vec![],
        });
    }
    assert_eq!(ctx.trait_defs().len(), 20);
    assert_eq!(ctx.impl_defs().len(), 20);
    let preds: Vec<_> = (0..20)
        .map(|i| TraitPredicate {
            trait_ref: TraitRef {
                def_id: TraitDefId::from_raw(1000 + i),
                substs: ty_ctx.intern_substitution(vec![]),
            },
            polarity: ImplPolarity::Positive,
        })
        .collect();
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&ctx);
    for pred in &preds {
        assert_eq!(solver.can_prove(&frozen, pred), SolverResult::Proven);
    }
}

#[test]
fn t34_evaluate_predicate_on_non_existent_trait() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    trait_ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(34),
        name: interner.intern("Ghost"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = ty_ctx.intern_substitution(vec![]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: TraitDefId::from_raw(9999),
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::DefiniteNo);
}

#[test]
fn t35_trait_context_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<TraitContext>();
    assert_sync::<TraitContext>();
}

#[test]
fn t36_solver_result_clone() {
    assert_eq!(SolverResult::Proven.clone(), SolverResult::Proven);
    assert_eq!(SolverResult::Ambiguous.clone(), SolverResult::Ambiguous);
    assert_eq!(SolverResult::DefiniteNo.clone(), SolverResult::DefiniteNo);
}

#[test]
fn t37_trait_def_clone() {
    let interner = Interner::new();
    let td = TraitDef {
        def_id: TraitDefId::from_raw(37),
        name: interner.intern("Clone37"),
        associated_types: vec![],
        predicates: vec![],
    };
    let td2 = td.clone();
    assert_eq!(td.def_id, td2.def_id);
    assert_eq!(td.name, td2.name);
}

#[test]
fn t38_impl_def_clone() {
    let mut ty_ctx = test_ty_ctx();
    let subst = ty_ctx.intern_substitution(vec![]);
    let imp = ImplDef {
        def_id: ImplDefId::from_raw(38),
        trait_ref: TraitRef {
            def_id: TraitDefId::from_raw(1),
            substs: subst,
        },
        predicates: vec![],
    };
    let imp2 = imp.clone();
    assert_eq!(imp.def_id, imp2.def_id);
    assert_eq!(imp.trait_ref.def_id, imp2.trait_ref.def_id);
}

#[test]
fn t48_solver_does_not_mutate_trait_ctx() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(48);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Immutable"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(4800),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });
    let pred_subst = ty_ctx.intern_substitution(vec![]);
    let frozen = ty_ctx.freeze();
    let defs_before = trait_ctx.trait_defs().len();
    let impls_before = trait_ctx.impl_defs().len();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let _ = solver.can_prove(
        &frozen,
        &TraitPredicate {
            trait_ref: TraitRef {
                def_id: trait_id,
                substs: pred_subst,
            },
            polarity: ImplPolarity::Positive,
        },
    );
    assert_eq!(trait_ctx.trait_defs().len(), defs_before);
    assert_eq!(trait_ctx.impl_defs().len(), impls_before);
}

#[test]
fn t49_impls_of_trait_returns_exact_count() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let tid_a = TraitDefId::from_raw(49);
    let tid_b = TraitDefId::from_raw(50);
    ctx.register_trait(TraitDef {
        def_id: tid_a,
        name: interner.intern("A"),
        associated_types: vec![],
        predicates: vec![],
    });
    ctx.register_trait(TraitDef {
        def_id: tid_b,
        name: interner.intern("B"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    for i in 0..7 {
        ctx.register_impl(ImplDef {
            def_id: ImplDefId::from_raw(4900 + i),
            trait_ref: TraitRef {
                def_id: tid_a,
                substs: ty_ctx.intern_substitution(vec![]),
            },
            predicates: vec![],
        });
    }
    for i in 0..3 {
        ctx.register_impl(ImplDef {
            def_id: ImplDefId::from_raw(5000 + i),
            trait_ref: TraitRef {
                def_id: tid_b,
                substs: ty_ctx.intern_substitution(vec![]),
            },
            predicates: vec![],
        });
    }
    assert_eq!(ctx.impls_of_trait(tid_a).count(), 7);
    assert_eq!(ctx.impls_of_trait(tid_b).count(), 3);
}

#[test]
fn t50_prove_with_substitution_same_as_empty() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let tid = TraitDefId::from_raw(50);
    ctx.register_trait(TraitDef {
        def_id: tid,
        name: interner.intern("Subst"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(5000),
        trait_ref: TraitRef {
            def_id: tid,
            substs: subst,
        },
        predicates: vec![],
    });
    let pred_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&ctx);
    assert_eq!(
        solver.can_prove(
            &frozen,
            &TraitPredicate {
                trait_ref: TraitRef {
                    def_id: tid,
                    substs: pred_subst
                },
                polarity: ImplPolarity::Positive
            }
        ),
        SolverResult::Proven
    );
}

#[test]
fn t59_solver_with_empty_trait_context_always_definite_no() {
    let ctx = TraitContext::new();
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&ctx);
    for i in 0..10 {
        let pred = TraitPredicate {
            trait_ref: TraitRef {
                def_id: TraitDefId::from_raw(i),
                substs: subst,
            },
            polarity: ImplPolarity::Positive,
        };
        assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::DefiniteNo);
    }
}

#[test]
fn t60_register_impl_for_unregistered_trait() {
    let _interner = Interner::new();
    let mut ctx = TraitContext::new();
    let mut ty_ctx = test_ty_ctx();
    let subst = ty_ctx.intern_substitution(vec![]);
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6000),
        trait_ref: TraitRef {
            def_id: TraitDefId::from_raw(999),
            substs: subst,
        },
        predicates: vec![],
    });
    assert_eq!(ctx.impl_defs().len(), 1);
    assert_eq!(ctx.impls_of_trait(TraitDefId::from_raw(999)).count(), 1);
}

#[test]
fn t61_trait_def_with_many_associated_types() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let assoc: Vec<_> = (0..10)
        .map(|i| interner.intern(&format!("Assoc{}", i)))
        .collect();
    ctx.register_trait(TraitDef {
        def_id: TraitDefId::from_raw(61),
        name: interner.intern("ManyAssoc"),
        associated_types: assoc.clone(),
        predicates: vec![],
    });
    assert_eq!(ctx.trait_defs()[0].associated_types.len(), 10);
}

#[test]
fn t62_impl_def_with_many_predicates() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(62);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("ManyPreds"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = ty_ctx.intern_substitution(vec![]);
    let preds: Vec<_> = (0..5)
        .map(|_| glyim_type::Predicate::WellFormed(ty_ctx.bool_ty()))
        .collect();
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6200),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: preds,
    });
    assert_eq!(trait_ctx.impl_defs()[0].predicates.len(), 5);
}

#[test]
fn t63_prove_checks_matching_not_existence() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let tid = TraitDefId::from_raw(63);
    ctx.register_trait(TraitDef {
        def_id: tid,
        name: interner.intern("Match"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let f64_ty = ty_ctx.mk_ty(glyim_type::TyKind::Float(glyim_core::FloatTy::F64));

    let subst_i32 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6301),
        trait_ref: TraitRef {
            def_id: tid,
            substs: subst_i32,
        },
        predicates: vec![],
    });

    let subst_f64 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(f64_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&ctx);
    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: tid,
            substs: subst_f64,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::DefiniteNo);
}

#[test]
fn t64_solver_evaluate_predicate_all_variants() {
    let interner = Interner::new();
    let mut ctx = TraitContext::new();
    let tid = TraitDefId::from_raw(64);
    ctx.register_trait(TraitDef {
        def_id: tid,
        name: interner.intern("All"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = ty_ctx.intern_substitution(vec![]);
    ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6400),
        trait_ref: TraitRef {
            def_id: tid,
            substs: subst,
        },
        predicates: vec![],
    });
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&ctx);
    let trait_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: tid,
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.evaluate_predicate(&frozen, &Predicate::Trait(trait_pred)),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(&frozen, &Predicate::WellFormed(frozen.bool_ty())),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(
            &frozen,
            &Predicate::TypeOutlives(glyim_type::TypeOutlivesPredicate {
                ty: frozen.bool_ty(),
                region: glyim_type::Region::Static
            })
        ),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(
            &frozen,
            &Predicate::RegionOutlives(glyim_type::RegionOutlivesPredicate {
                a: glyim_type::Region::Static,
                b: glyim_type::Region::Static
            })
        ),
        SolverResult::Proven
    );
    assert_eq!(
        solver.evaluate_predicate(
            &frozen,
            &Predicate::Coerce(frozen.bool_ty(), frozen.unit_ty())
        ),
        SolverResult::Ambiguous
    );
}

#[test]
fn t65_prove_negative_impl_with_positive_existing() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(65);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("NegativeTest"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    let pred_subst = empty_subst(&mut ty_ctx);

    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6500),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });

    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let neg_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Negative,
    };
    assert_eq!(
        solver.can_prove(&frozen, &neg_pred),
        SolverResult::DefiniteNo
    );
}

#[test]
fn t66_prove_negative_impl_with_no_impls() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(66);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("NegativeTestNoImpls"),
        associated_types: vec![],
        predicates: vec![],
    });
    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let neg_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        polarity: ImplPolarity::Negative,
    };
    assert_eq!(
        solver.can_prove(&frozen, &neg_pred),
        SolverResult::Ambiguous
    );
}

#[test]
fn t67_prove_with_where_clause_proven() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(67);
    let base_trait_id = TraitDefId::from_raw(670);

    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Derived"),
        associated_types: vec![],
        predicates: vec![],
    });
    trait_ctx.register_trait(TraitDef {
        def_id: base_trait_id,
        name: interner.intern("Base"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);

    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6700),
        trait_ref: TraitRef {
            def_id: base_trait_id,
            substs: subst,
        },
        predicates: vec![],
    });

    let base_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: base_trait_id,
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6701),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![Predicate::Trait(base_pred)],
    });

    let pred_subst = empty_subst(&mut ty_ctx);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let derived_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.can_prove(&frozen, &derived_pred),
        SolverResult::Proven
    );
}

#[test]
fn t68_prove_with_where_clause_fails() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(68);
    let base_trait_id = TraitDefId::from_raw(680);

    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("DerivedFail"),
        associated_types: vec![],
        predicates: vec![],
    });
    trait_ctx.register_trait(TraitDef {
        def_id: base_trait_id,
        name: interner.intern("BaseFail"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);

    let base_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: base_trait_id,
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6801),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![Predicate::Trait(base_pred)],
    });

    let pred_subst = empty_subst(&mut ty_ctx);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let derived_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.can_prove(&frozen, &derived_pred),
        SolverResult::DefiniteNo
    );
}

#[test]
fn t69_substitution_mismatch_yields_definite_no() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(69);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("MatchTy"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let f64_ty = ty_ctx.mk_ty(glyim_type::TyKind::Float(glyim_core::FloatTy::F64));

    let subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(6900),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });

    let pred_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(f64_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::DefiniteNo);
}

#[test]
fn t70_generic_param_matches_anything() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(70);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("GenericParam"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let param_ty = ty_ctx.mk_ty(glyim_type::TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: interner.intern("T"),
    }));
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));

    let impl_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(param_ty)]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7000),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    });

    let pred_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::Proven);
}

#[test]
fn t71_projection_predicate_proven() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(71);
    let assoc_name = interner.intern("Item");

    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("AssocTrait"),
        associated_types: vec![assoc_name],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7100),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![],
    });

    let proj = glyim_type::ProjectionTy {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        item_name: assoc_name,
    };
    let proj_ty = ty_ctx.mk_ty(glyim_type::TyKind::Projection(proj));

    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let result = solver.evaluate_predicate(&frozen, &Predicate::WellFormed(proj_ty));
    assert_eq!(result, SolverResult::Proven);
}

#[test]
fn t72_coherence_overlap_violation() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(72);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Overlap"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let param_a = ty_ctx.mk_ty(glyim_type::TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: interner.intern("T"),
    }));

    let subst_a = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(param_a)]);

    // Use identical substitution to properly trigger the naive overlap check
    let subst_b = subst_a;

    let impl1 = ImplDef {
        def_id: ImplDefId::from_raw(7200),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst_a,
        },
        predicates: vec![],
    };
    trait_ctx.register_impl(impl1.clone());

    let impl2 = ImplDef {
        def_id: ImplDefId::from_raw(7201),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst_b,
        },
        predicates: vec![],
    };

    let coherence_result = trait_ctx.check_coherence(&impl2);
    assert!(coherence_result.is_err());
    assert!(coherence_result.unwrap_err().contains("Overlap violation"));
}

#[test]
fn t73_coherence_no_overlap_different_types() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(73);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("NoOverlap"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let f64_ty = ty_ctx.mk_ty(glyim_type::TyKind::Float(glyim_core::FloatTy::F64));

    let subst_i32 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let subst_f64 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(f64_ty)]);

    let impl1 = ImplDef {
        def_id: ImplDefId::from_raw(7300),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst_i32,
        },
        predicates: vec![],
    };
    trait_ctx.register_impl(impl1);

    let impl2 = ImplDef {
        def_id: ImplDefId::from_raw(7301),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst_f64,
        },
        predicates: vec![],
    };

    let coherence_result = trait_ctx.check_coherence(&impl2);
    assert!(coherence_result.is_ok());
}

#[test]
fn t74_multiple_matching_impls_ambiguous() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(74);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("AmbigImpl"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let param_t = ty_ctx.mk_ty(glyim_type::TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: interner.intern("T"),
    }));

    let subst1 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(param_t)]);
    let subst2 = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(param_t)]);

    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7400),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst1,
        },
        predicates: vec![],
    });
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7401),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst2,
        },
        predicates: vec![],
    });

    let query_subst =
        ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(ty_ctx.bool_ty())]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: query_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::Ambiguous);
}

#[test]
fn t75_substs_length_mismatch() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(75);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("LenMismatch"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let bool_ty = ty_ctx.bool_ty();

    let impl_subst = ty_ctx.intern_substitution(vec![
        glyim_type::GenericArg::Ty(i32_ty),
        glyim_type::GenericArg::Ty(bool_ty),
    ]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7500),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    });

    let query_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: query_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::DefiniteNo);
}

#[test]
fn t76_infer_var_in_query_matches() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(76);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("InferMatch"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let infer_ty = ty_ctx.mk_ty(glyim_type::TyKind::Infer(glyim_type::InferVar::Ty(
        glyim_type::TyVar::from_raw(0),
    )));

    let impl_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7600),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    });

    let query_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(infer_ty)]);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    let pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: query_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(solver.can_prove(&frozen, &pred), SolverResult::Proven);
}

#[test]
fn t77_ref_substitution_matching() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(77);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("RefMatch"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let ref_i32 = ty_ctx.mk_ref(
        glyim_type::Region::Erased,
        i32_ty,
        glyim_core::Mutability::Not,
    );
    let ref_bool = ty_ctx.mk_ref(
        glyim_type::Region::Erased,
        ty_ctx.bool_ty(),
        glyim_core::Mutability::Not,
    );

    let impl_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(ref_i32)]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7700),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    });

    let query_no = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(ref_bool)]);
    let pred_no = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: query_no,
        },
        polarity: ImplPolarity::Positive,
    };

    let ref_i32_query = ty_ctx.mk_ref(
        glyim_type::Region::Erased,
        i32_ty,
        glyim_core::Mutability::Not,
    );
    let query_yes = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(ref_i32_query)]);
    let pred_yes = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: query_yes,
        },
        polarity: ImplPolarity::Positive,
    };

    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);

    assert_eq!(
        solver.can_prove(&frozen, &pred_no),
        SolverResult::DefiniteNo
    );
    assert_eq!(solver.can_prove(&frozen, &pred_yes), SolverResult::Proven);
}

#[test]
fn t78_where_clause_ambiguous() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(78);
    let base_trait_id = TraitDefId::from_raw(780);

    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("DerivedAmb"),
        associated_types: vec![],
        predicates: vec![],
    });
    trait_ctx.register_trait(TraitDef {
        def_id: base_trait_id,
        name: interner.intern("BaseAmb"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let subst = empty_subst(&mut ty_ctx);

    let param_t = ty_ctx.mk_ty(glyim_type::TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: interner.intern("T"),
    }));
    let impl_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(param_t)]);
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7800),
        trait_ref: TraitRef {
            def_id: base_trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    });

    let base_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: base_trait_id,
            substs: subst,
        },
        polarity: ImplPolarity::Positive,
    };
    trait_ctx.register_impl(ImplDef {
        def_id: ImplDefId::from_raw(7801),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: subst,
        },
        predicates: vec![Predicate::Trait(base_pred)],
    });

    let pred_subst = empty_subst(&mut ty_ctx);
    let frozen = ty_ctx.freeze();
    let mut solver = SimpleTraitSolver::new(&trait_ctx);
    let derived_pred = TraitPredicate {
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: pred_subst,
        },
        polarity: ImplPolarity::Positive,
    };
    assert_eq!(
        solver.can_prove(&frozen, &derived_pred),
        SolverResult::DefiniteNo
    );
}

#[test]
fn t79_coherence_orphan_rule_violation() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(79);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("Orphan"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let impl_subst = ty_ctx.intern_substitution(vec![]);
    let impl_def = ImplDef {
        def_id: ImplDefId::from_raw(7900),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    };

    let result = trait_ctx.check_coherence(&impl_def);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Orphan rule violation"));
}

#[test]
fn t80_coherence_orphan_rule_pass() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    let trait_id = TraitDefId::from_raw(80);
    trait_ctx.register_trait(TraitDef {
        def_id: trait_id,
        name: interner.intern("OrphanPass"),
        associated_types: vec![],
        predicates: vec![],
    });

    let mut ty_ctx = test_ty_ctx();
    let i32_ty = ty_ctx.mk_ty(glyim_type::TyKind::Int(glyim_core::IntTy::I32));
    let impl_subst = ty_ctx.intern_substitution(vec![glyim_type::GenericArg::Ty(i32_ty)]);
    let impl_def = ImplDef {
        def_id: ImplDefId::from_raw(8000),
        trait_ref: TraitRef {
            def_id: trait_id,
            substs: impl_subst,
        },
        predicates: vec![],
    };

    let result = trait_ctx.check_coherence(&impl_def);
    assert!(result.is_ok());
}
