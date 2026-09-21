//! Tests for `dyn Trait` resolution (plan §1.5 / dynamic dispatch foundation).
//!
//! `dyn Trait` must resolve to a `TyKind::Dynamic` value carrying the trait
//! predicate, and an object-safety violation must be reported when the trait
//! is not object-safe (e.g. a method taking `self` by value).

use crate::tests::test_utils::global_interner;
use crate::tyconv::resolve_type_ref;
use glyim_core::def_id::{CrateId, LocalDefId, TraitDefId};
use glyim_core::interner::Interner;
use glyim_core::primitives::{Abi, Safety, Visibility};
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
use glyim_hir::{Path, TypeRef};
use glyim_solve::InferenceTable;
use glyim_span::Span;
use glyim_type::object_safety::{MethodSelfKind, MethodSignature, TraitObjectSafetyInput};
use glyim_type::{
    GenericArg, ImplPolarity, Predicate, Region, TraitDef, TraitPredicate, TraitRef, Ty, TyKind,
};
use std::collections::HashMap;

/// Build a def map that resolves `trait_name` to a `TraitDefId`.
fn def_map_with_trait(interner: &mut Interner, trait_name: &str) -> (CrateDefMap, TraitDefId) {
    let name = interner.intern(trait_name);
    let mut scope = ItemScope::default();
    // The trait name resolves (single-segment) to a LocalDefId in the root scope.
    scope.types.insert(
        name,
        (LocalDefId::from_raw(0), Visibility::Public, Span::DUMMY),
    );
    let root_id = ModuleId::from_raw(0);
    let root_data = ModuleData {
        parent: None,
        children: vec![],
        scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };
    let mut modules = glyim_core::arena::IndexVec::new();
    modules.push(root_data);
    let def_map = CrateDefMap {
        root: root_id,
        modules,
        krate: CrateId::from_raw(0),
        interner: interner.clone(),
        variant_map: Default::default(),
        max_local_def_id: 0,
    };
    (def_map, TraitDefId::from_raw(0))
}

#[test]
fn dyn_trait_resolves_to_dynamic_type() {
    let mut inter = global_interner();
    let trait_name = "Animal";
    let (def_map, trait_def_id) = def_map_with_trait(&mut inter, trait_name);

    let mut ctx = glyim_type::TyCtxMut::new(inter.clone());
    // Register a trait with a single `&self` method → object-safe.
    let self_ref_ty = ctx.mk_ref(
        Region::Erased,
        Ty::UNIT,
        glyim_core::primitives::Mutability::Not,
    );
    let method_sig = glyim_type::FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(self_ref_ty)]),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx.register_trait_def(
        trait_def_id,
        TraitDef {
            name: inter.intern(trait_name),
            methods: vec![glyim_type::MethodDef {
                name: inter.intern("speak"),
                sig: method_sig,
                fn_def_id: None,
            }],
            associated_types: vec![],
        },
    );

    let mut infer = InferenceTable::new();
    let mut diagnostics = Vec::new();
    let param_map: HashMap<glyim_core::Name, Ty> = HashMap::new();

    let dyn_ty = resolve_type_ref(
        &mut ctx,
        &mut infer,
        &def_map,
        &mut diagnostics,
        &TypeRef::Dyn(Box::new(TypeRef::Path(Path::from_single(
            inter.intern(trait_name),
        )))),
        &param_map,
        Span::DUMMY,
    );

    // No diagnostics: the trait is object-safe.
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        diagnostics
    );

    match ctx.ty_kind(dyn_ty) {
        TyKind::Dynamic(binder, Region::Erased) => {
            let preds = binder.as_ref().skip_binder();
            assert_eq!(
                preds.len(),
                1,
                "dyn should carry exactly one trait predicate"
            );
            match &preds[0] {
                Predicate::Trait(TraitPredicate {
                    trait_ref: TraitRef { def_id, .. },
                    polarity: ImplPolarity::Positive,
                }) => {
                    assert_eq!(*def_id, trait_def_id, "predicate must reference the trait");
                }
                other => panic!("expected Trait predicate, got {:?}", other),
            }
        }
        other => panic!("dyn Trait must resolve to TyKind::Dynamic, got {:?}", other),
    }
}

#[test]
fn dyn_trait_non_object_safe_reports_diagnostic() {
    let inter = global_interner();
    let trait_name = "ByValueTrait";
    let (def_map, trait_def_id) = {
        let name = inter.intern(trait_name);
        let mut scope = ItemScope::default();
        scope.types.insert(
            name,
            (LocalDefId::from_raw(0), Visibility::Public, Span::DUMMY),
        );
        let root_id = ModuleId::from_raw(0);
        let root_data = ModuleData {
            parent: None,
            children: vec![],
            scope,
            origin: ModuleOrigin::CrateRoot,
            span: Span::DUMMY,
            def_id: LocalDefId::from_raw(0),
            visibility: Visibility::Public,
        };
        let mut modules = glyim_core::arena::IndexVec::new();
        modules.push(root_data);
        (
            CrateDefMap {
                root: root_id,
                modules,
                krate: CrateId::from_raw(0),
                interner: inter.clone(),
                variant_map: Default::default(),
                max_local_def_id: 0,
            },
            TraitDefId::from_raw(0),
        )
    };

    let mut ctx = glyim_type::TyCtxMut::new(inter.clone());
    // A method taking `self` by value → NOT object-safe.
    let method_sig = glyim_type::FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(Ty::UNIT)]),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx.register_trait_def(
        trait_def_id,
        TraitDef {
            name: inter.intern(trait_name),
            methods: vec![glyim_type::MethodDef {
                name: inter.intern("consume"),
                sig: method_sig,
                fn_def_id: None,
            }],
            associated_types: vec![],
        },
    );

    let mut infer = InferenceTable::new();
    let mut diagnostics = Vec::new();
    let param_map: HashMap<glyim_core::Name, Ty> = HashMap::new();

    let dyn_ty = resolve_type_ref(
        &mut ctx,
        &mut infer,
        &def_map,
        &mut diagnostics,
        &TypeRef::Dyn(Box::new(TypeRef::Path(Path::from_single(
            inter.intern(trait_name),
        )))),
        &param_map,
        Span::DUMMY,
    );

    assert!(
        !diagnostics.is_empty(),
        "expected an object-safety diagnostic for a by-value-self trait"
    );
    // The type is still produced (resolve_type_ref returns a Dynamic ty), but
    // the diagnostic explains the trait is not object-safe.
    assert!(matches!(ctx.ty_kind(dyn_ty), TyKind::Dynamic(..)));
}

// Smoke test that the object-safety helper used by the resolver agrees with the
// dedicated algorithm tests in glyim-type.
#[test]
fn object_safety_helper_sanity() {
    let violations = glyim_type::object_safety::check_object_safety(&TraitObjectSafetyInput {
        requires_self_sized: false,
        methods: &[MethodSignature {
            name: glyim_core::interner::Interner::new().intern("m"),
            span: Span::DUMMY,
            self_kind: MethodSelfKind::ByValue,
            has_generic_params: false,
            returns_self: false,
        }],
        associated_types: &[],
        supertrait_safety: &[],
    });
    assert!(
        violations.iter().any(|v| matches!(
            v,
            glyim_type::object_safety::ObjectSafetyViolation::ByValueSelf { .. }
        )),
        "by-value self method must be flagged"
    );
}

/// Build a def map where `mod_name::trait_name` is resolvable: a child module
/// whose scope contains the trait name, mapped to `trait_local_id`.
fn def_map_with_nested_trait(
    interner: &mut Interner,
    mod_name: &str,
    trait_name: &str,
    trait_local_id: u32,
) -> CrateDefMap {
    let mod_n = interner.intern(mod_name);
    let trait_n = interner.intern(trait_name);

    // Root module: no direct trait, but a child named `mod_name`.
    let mut root_scope = ItemScope::default();
    let root_id = ModuleId::from_raw(0);
    let child_id = ModuleId::from_raw(1);
    root_scope.types.insert(
        mod_n,
        (LocalDefId::from_raw(0), Visibility::Public, Span::DUMMY),
    );

    let root_data = ModuleData {
        parent: None,
        children: vec![(mod_n, child_id)],
        scope: root_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(0),
        visibility: Visibility::Public,
    };

    // Child module: contains the trait name.
    let mut child_scope = ItemScope::default();
    child_scope.types.insert(
        trait_n,
        (
            LocalDefId::from_raw(trait_local_id),
            Visibility::Public,
            Span::DUMMY,
        ),
    );
    let child_data = ModuleData {
        parent: Some(root_id),
        children: vec![],
        scope: child_scope,
        origin: ModuleOrigin::CrateRoot,
        span: Span::DUMMY,
        def_id: LocalDefId::from_raw(1),
        visibility: Visibility::Public,
    };

    let mut modules = glyim_core::arena::IndexVec::new();
    modules.push(root_data);
    modules.push(child_data);

    CrateDefMap {
        root: root_id,
        modules,
        krate: CrateId::from_raw(0),
        interner: interner.clone(),
        variant_map: Default::default(),
        max_local_def_id: 0,
    }
}

#[test]
fn dyn_trait_multi_segment_path_resolves() {
    let mut inter = global_interner();
    let trait_name = "Animal";
    let mod_name = "zoo";
    // The child module maps `Animal` → LocalDefId(7); the registered TraitDefId
    // must match so resolution lines up.
    let trait_local_id = 7u32;
    let def_map = def_map_with_nested_trait(&mut inter, mod_name, trait_name, trait_local_id);
    let trait_def_id = TraitDefId::from_raw(trait_local_id);

    let mut ctx = glyim_type::TyCtxMut::new(inter.clone());
    let self_ref_ty = ctx.mk_ref(
        Region::Erased,
        Ty::UNIT,
        glyim_core::primitives::Mutability::Not,
    );
    let method_sig = glyim_type::FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(self_ref_ty)]),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    ctx.register_trait_def(
        trait_def_id,
        TraitDef {
            name: inter.intern(trait_name),
            methods: vec![glyim_type::MethodDef {
                name: inter.intern("speak"),
                sig: method_sig,
                fn_def_id: None,
            }],
            associated_types: vec![],
        },
    );

    let mut infer = InferenceTable::new();
    let mut diagnostics = Vec::new();
    let param_map: HashMap<glyim_core::Name, Ty> = HashMap::new();

    // `dyn zoo::Animal` — a multi-segment trait path.
    let mut path = Path::from_single(inter.intern(trait_name));
    path.kind = glyim_core::path::PathKind::Plain;
    path.segments.insert(
        0,
        glyim_hir::PathSegment {
            name: inter.intern(mod_name),
            generic_args: None,
        },
    );

    let dyn_ty = resolve_type_ref(
        &mut ctx,
        &mut infer,
        &def_map,
        &mut diagnostics,
        &TypeRef::Dyn(Box::new(TypeRef::Path(path))),
        &param_map,
        Span::DUMMY,
    );

    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for multi-segment object-safe trait, got: {:?}",
        diagnostics
    );
    match ctx.ty_kind(dyn_ty) {
        TyKind::Dynamic(binder, Region::Erased) => {
            let preds = binder.as_ref().skip_binder();
            assert_eq!(
                preds.len(),
                1,
                "dyn should carry exactly one trait predicate"
            );
            match &preds[0] {
                Predicate::Trait(TraitPredicate {
                    trait_ref: TraitRef { def_id, .. },
                    polarity: ImplPolarity::Positive,
                }) => {
                    assert_eq!(
                        *def_id, trait_def_id,
                        "predicate must reference the nested trait"
                    );
                }
                other => panic!("expected Trait predicate, got {:?}", other),
            }
        }
        other => panic!(
            "multi-segment dyn Trait must resolve to TyKind::Dynamic, got {:?}",
            other
        ),
    }
}

/// Multi-segment ADT paths (e.g. `zoo::Animal`) must resolve through the module
/// tree to a `TyKind::Adt`, not just crate-root names.
#[test]
fn adt_multi_segment_path_resolves_to_adt() {
    let mut inter = global_interner();
    let mod_name = "zoo";
    let adt_name = "Animal";
    let adt_local_id = 7u32;
    let def_map = def_map_with_nested_trait(&mut inter, mod_name, adt_name, adt_local_id);

    let mut ctx = glyim_type::TyCtxMut::new(inter.clone());
    let mut infer = InferenceTable::new();
    let mut diagnostics = Vec::new();
    let param_map: HashMap<glyim_core::Name, Ty> = HashMap::new();

    // `zoo::Animal` — a multi-segment ADT path (no traits registered).
    let mut path = Path::from_single(inter.intern(adt_name));
    path.kind = glyim_core::path::PathKind::Plain;
    path.segments.insert(
        0,
        glyim_hir::PathSegment {
            name: inter.intern(mod_name),
            generic_args: None,
        },
    );

    let ty = resolve_type_ref(
        &mut ctx,
        &mut infer,
        &def_map,
        &mut diagnostics,
        &TypeRef::Path(path),
        &param_map,
        Span::DUMMY,
    );

    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for multi-segment ADT path, got: {:?}",
        diagnostics
    );
    match ctx.ty_kind(ty) {
        TyKind::Adt(adt_id, substs) => {
            assert_eq!(
                adt_id.index(),
                adt_local_id as usize,
                "must resolve to the nested ADT"
            );
            assert_eq!(
                ctx.substitution_args(*substs).len(),
                0,
                "ADT without generic args has empty substs"
            );
        }
        other => panic!(
            "multi-segment ADT path must resolve to TyKind::Adt, got {:?}",
            other
        ),
    }
}
