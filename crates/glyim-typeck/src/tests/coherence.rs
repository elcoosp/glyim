//! Coherence checker tests.

use crate::coherence::{CoherenceChecker, ResolvedImplHeader};
use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, LocalDefId, TraitDefId};
use glyim_core::interner::Interner;
use glyim_core::primitives::{FloatTy, IntTy, UintTy, Visibility};
use glyim_def_map::{CrateDefMap, ItemScope, ModuleData, ModuleId, ModuleOrigin};
use glyim_hir::{GenericParam, GenericParamKind, ImplItem, Path, TypeRef};
use glyim_span::Span;
use glyim_type::{ImplPolarity, ParamTy, Substitution, Ty, TyCtxMut, TyKind};

// ---- Helpers ----

fn global_interner() -> Interner {
    Interner::new()
}

fn make_ty_ctx() -> TyCtxMut {
    TyCtxMut::new(global_interner())
}

fn type_ref_to_ty(
    type_ref: &TypeRef,
    interner: &Interner,
    ctx: &mut TyCtxMut,
    def_map: &CrateDefMap,
) -> Ty {
    match type_ref {
        TypeRef::Path(p) => {
            let name = p.as_name().or_else(|| p.segments.first().map(|s| s.name));
            let name = match name {
                Some(n) => n,
                None => return Ty::ERROR,
            };
            let is_generic = false; // self-type params aren't expressed here
            let _ = is_generic;
            if let Some(seg) = p.segments.first()
                && let Some(args) = &seg.generic_args
            {
                // Generic ADT: `Name<Arg1, Arg2, ...>`.
                let adt_id = match def_map.modules[def_map.root].scope.resolve(name) {
                    Some(res) => glyim_core::def_id::AdtId::from_raw(res.0.to_raw()),
                    None => return Ty::ERROR,
                };
                let generic_args: Vec<glyim_type::GenericArg> = args
                    .iter()
                    .map(|a| glyim_type::GenericArg::Ty(type_ref_to_ty(a, interner, ctx, def_map)))
                    .collect();
                let substs = ctx.intern_substitution(generic_args);
                return ctx.mk_ty(TyKind::Adt(adt_id, substs));
            }
            if let Some(res) = def_map.modules[def_map.root].scope.resolve(name) {
                let adt_id = glyim_core::def_id::AdtId::from_raw(res.0.to_raw());
                let substs = ctx.intern_substitution(vec![]);
                ctx.mk_ty(TyKind::Adt(adt_id, substs))
            } else {
                let s = interner.resolve(name);
                match s {
                    "i8" => ctx.mk_ty(TyKind::Int(IntTy::I8)),
                    "i16" => ctx.mk_ty(TyKind::Int(IntTy::I16)),
                    "i32" => ctx.mk_ty(TyKind::Int(IntTy::I32)),
                    "i64" => ctx.mk_ty(TyKind::Int(IntTy::I64)),
                    "isize" => ctx.mk_ty(TyKind::Int(IntTy::Isize)),
                    "u8" => ctx.mk_ty(TyKind::Uint(UintTy::U8)),
                    "u16" => ctx.mk_ty(TyKind::Uint(UintTy::U16)),
                    "u32" => ctx.mk_ty(TyKind::Uint(UintTy::U32)),
                    "u64" => ctx.mk_ty(TyKind::Uint(UintTy::U64)),
                    "usize" => ctx.mk_ty(TyKind::Uint(UintTy::Usize)),
                    "f32" => ctx.mk_ty(TyKind::Float(FloatTy::F32)),
                    "f64" => ctx.mk_ty(TyKind::Float(FloatTy::F64)),
                    "bool" => Ty::BOOL,
                    _ => Ty::ERROR,
                }
            }
        }
        _ => Ty::ERROR,
    }
}

fn impl_item_to_header(
    impl_item: &ImplItem,
    _interner: &mut Interner,
    ctx: &mut TyCtxMut,
    def_map: &CrateDefMap,
) -> ResolvedImplHeader {
    let trait_name = impl_item.trait_ref.as_ref().and_then(|p| p.as_name());

    let trait_def_id = if let Some(name) = trait_name {
        def_map.modules[def_map.root]
            .scope
            .resolve(name)
            .map(|res| TraitDefId::from_raw(res.0.to_raw()))
    } else {
        None
    };

    // Generic self-type parameters (e.g. blanket `impl Trait for T`) must
    // become `TyKind::Param`, not be resolved as a named type.
    let self_ty = if let TypeRef::Path(p) = &impl_item.self_ty {
        if let Some(name) = p.as_name() {
            let is_generic = impl_item.generic_params.iter().any(|gp| gp.name == name);
            if is_generic {
                let idx = impl_item
                    .generic_params
                    .iter()
                    .position(|gp| gp.name == name)
                    .unwrap() as u32;
                ctx.mk_ty(TyKind::Param(ParamTy { index: idx, name }))
            } else {
                type_ref_to_ty(&impl_item.self_ty, _interner, ctx, def_map)
            }
        } else {
            type_ref_to_ty(&impl_item.self_ty, _interner, ctx, def_map)
        }
    } else {
        type_ref_to_ty(&impl_item.self_ty, _interner, ctx, def_map)
    };

    let self_type_name = match &impl_item.self_ty {
        TypeRef::Path(p) => p
            .as_name()
            .filter(|name| def_map.modules[def_map.root].scope.resolve(*name).is_some()),
        _ => None,
    };

    let generic_param_names = impl_item.generic_params.iter().map(|p| p.name).collect();

    ResolvedImplHeader {
        trait_def_id,
        trait_name,
        trait_substs: Substitution::empty(),
        self_ty,
        self_type_name,
        generic_param_names,
        polarity: ImplPolarity::Positive,
        span: Span::DUMMY,
    }
}

fn build_def_map(
    interner: &mut Interner,
    krate: CrateId,
    local_type_names: &[&str],
) -> CrateDefMap {
    let mut scope = ItemScope::default();
    for (i, &name_str) in local_type_names.iter().enumerate() {
        let name = interner.intern(name_str);
        scope.types.insert(
            name,
            (
                LocalDefId::from_raw(i as u32),
                Visibility::Public,
                Span::DUMMY,
            ),
        );
    }
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
    let mut modules = IndexVec::new();
    modules.push(root_data);
    CrateDefMap {
        root: root_id,
        modules,
        krate,
        interner: interner.clone(),
        variant_map: Default::default(),
        max_local_def_id: 0,
    }
}

fn make_impl_item(interner: &mut Interner, trait_name: &str, self_ty_name: &str) -> ImplItem {
    let trait_path = Path::from_single(interner.intern(trait_name));
    let self_ty_path = Path::from_single(interner.intern(self_ty_name));
    ImplItem {
        trait_ref: Some(trait_path),
        self_ty: TypeRef::Path(self_ty_path),
        methods: vec![],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    }
}

fn make_blanket_impl_item(interner: &mut Interner, trait_name: &str, param_name: &str) -> ImplItem {
    let param = interner.intern(param_name);
    ImplItem {
        trait_ref: Some(Path::from_single(interner.intern(trait_name))),
        self_ty: TypeRef::Path(Path::from_single(param)),
        methods: vec![],
        generic_params: vec![GenericParam {
            name: param,
            kind: GenericParamKind::Type {
                default: None,
                bounds: Vec::new(),
            },
            span: Span::DUMMY,
        }],
        where_clauses: vec![],
        associated_types: vec![],
    }
}

// ---- Tests ----

#[test]
fn t01_duplicate_impl_should_error() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyType", "Send"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let impl1 = make_impl_item(&mut interner, "Send", "MyType");
    let impl2 = make_impl_item(&mut interner, "Send", "MyType");

    let result1 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl1, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(result1.is_ok(), "first impl should be accepted");

    let result2 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl2, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(result2.is_err(), "duplicate impl should be rejected");
    let errors = result2.unwrap_err();
    assert!(!errors.is_empty());
    let msg = &errors[0].message;
    assert!(msg.contains("conflict") || msg.contains("overlap") || msg.contains("duplicate"));
}

#[test]
fn t02_orphan_rule_foreign_trait_foreign_type_error() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &[]);
    let mut ctx = make_ty_ctx();
    let _infer = glyim_solve::InferenceTable::new();
    let checker = CoherenceChecker::new(&def_map);

    let impl_item = make_impl_item(&mut interner, "ForeignTrait", "ForeignType");
    let result = checker.check_orphan_rule(&impl_item_to_header(
        &impl_item,
        &mut interner,
        &mut ctx,
        &def_map,
    ));
    assert!(
        result.is_err(),
        "orphan rule should reject foreign trait + foreign type"
    );
    let errors = result.unwrap_err();
    assert!(errors[0].message.contains("orphan rule"));
}

#[test]
fn t03_blanket_impl_conflicts_with_concrete() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyTrait"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let concrete = make_impl_item(&mut interner, "MyTrait", "i32");
    let blanket = make_blanket_impl_item(&mut interner, "MyTrait", "T");

    checker
        .check_and_register_impl_compat(
            &impl_item_to_header(&concrete, &mut interner, &mut ctx, &def_map),
            ImplPolarity::Positive,
            &mut ctx,
            &mut infer,
        )
        .unwrap();

    let result = checker.check_and_register_impl_compat(
        &impl_item_to_header(&blanket, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(
        result.is_err(),
        "blanket impl should conflict with concrete"
    );
}

#[test]
fn t04_valid_orphan_foreign_trait_local_type() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["LocalType"]);
    let mut ctx = make_ty_ctx();
    let checker = CoherenceChecker::new(&def_map);

    let impl_item = make_impl_item(&mut interner, "ForeignTrait", "LocalType");
    let result = checker.check_orphan_rule(&impl_item_to_header(
        &impl_item,
        &mut interner,
        &mut ctx,
        &def_map,
    ));
    assert!(
        result.is_ok(),
        "orphan rule should accept foreign trait + local type"
    );
}

#[test]
fn t05_negative_impl_overrides_auto_trait() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyType", "Send"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let neg_impl = make_impl_item(&mut interner, "Send", "MyType");
    let result = checker.check_and_register_impl_compat(
        &impl_item_to_header(&neg_impl, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Negative,
        &mut ctx,
        &mut infer,
    );
    assert!(result.is_ok(), "negative impl should be allowed");
}

#[test]
fn t06_duplicate_with_different_polarity_error() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyType", "Send"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let pos_impl = make_impl_item(&mut interner, "Send", "MyType");
    let neg_impl = make_impl_item(&mut interner, "Send", "MyType");

    checker
        .check_and_register_impl_compat(
            &impl_item_to_header(&pos_impl, &mut interner, &mut ctx, &def_map),
            ImplPolarity::Positive,
            &mut ctx,
            &mut infer,
        )
        .unwrap();

    let result = checker.check_and_register_impl_compat(
        &impl_item_to_header(&neg_impl, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Negative,
        &mut ctx,
        &mut infer,
    );
    assert!(
        result.is_err(),
        "impl with opposite polarity should conflict"
    );
}

#[test]
fn t07_orphan_local_trait_foreign_type_allowed() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyTrait"]);
    let mut ctx = make_ty_ctx();
    let checker = CoherenceChecker::new(&def_map);

    let impl_item = make_impl_item(&mut interner, "MyTrait", "ForeignType");
    let result = checker.check_orphan_rule(&impl_item_to_header(
        &impl_item,
        &mut interner,
        &mut ctx,
        &def_map,
    ));
    assert!(
        result.is_ok(),
        "orphan rule should allow local trait on foreign type"
    );
}

#[test]
fn t08_two_non_overlapping_blanket_impls_allowed() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["From"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let blanket_a = make_blanket_impl_item(&mut interner, "From", "A");
    let blanket_b = make_blanket_impl_item(&mut interner, "From", "B");

    let r1 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&blanket_a, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(r1.is_ok(), "first blanket impl should be accepted");

    let r2 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&blanket_b, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(
        r2.is_ok(),
        "second blanket impl with different param should be accepted"
    );
}

#[test]
fn t09_negative_impl_orphan_error() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &[]);
    let mut ctx = make_ty_ctx();
    let checker = CoherenceChecker::new(&def_map);

    let neg_impl = make_impl_item(&mut interner, "ForeignTrait", "ForeignType");
    let result = checker.check_orphan_rule(&impl_item_to_header(
        &neg_impl,
        &mut interner,
        &mut ctx,
        &def_map,
    ));
    assert!(
        result.is_err(),
        "negative impl for foreign trait + foreign type should violate orphan rule"
    );
}

#[test]
fn t10_different_traits_no_conflict() {
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["MyType", "TraitA", "TraitB"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let impl_trait_a = make_impl_item(&mut interner, "TraitA", "MyType");
    let impl_trait_b = make_impl_item(&mut interner, "TraitB", "MyType");

    let r1 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl_trait_a, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    let r2 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl_trait_b, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(r1.is_ok(), "impl for TraitA should be accepted");
    assert!(r2.is_ok(), "impl for TraitB should be accepted");
}

/// Build an `impl Trait for Adt<Args...>` impl item with generic self-type
/// arguments (e.g. `Vec<i32>`).
fn make_generic_impl_item(
    interner: &mut Interner,
    trait_name: &str,
    self_ty_name: &str,
    arg_names: &[&str],
) -> ImplItem {
    let arg_ty_refs: Vec<TypeRef> = arg_names
        .iter()
        .map(|n| TypeRef::Path(Path::from_single(interner.intern(n))))
        .collect();
    let self_ty_path = Path {
        segments: vec![glyim_hir::PathSegment {
            name: interner.intern(self_ty_name),
            generic_args: Some(arg_ty_refs),
        }],
        kind: glyim_core::path::PathKind::Plain,
    };
    ImplItem {
        trait_ref: Some(Path::from_single(interner.intern(trait_name))),
        self_ty: glyim_hir::TypeRef::Path(self_ty_path),
        methods: vec![],
        generic_params: vec![],
        where_clauses: vec![],
        associated_types: vec![],
    }
}

#[test]
fn t11_distinct_generic_args_do_not_overlap() {
    // Regression for Tier 2.1: `impl Foo for Vec<i32>` and
    // `impl Foo for Vec<String>` must NOT be treated as overlapping.
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["Foo", "Vec", "String"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let impl_i32 = make_generic_impl_item(&mut interner, "Foo", "Vec", &["i32"]);
    let impl_string = make_generic_impl_item(&mut interner, "Foo", "Vec", &["String"]);

    let r1 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl_i32, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(r1.is_ok(), "impl Foo for Vec<i32> should be accepted");

    let r2 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl_string, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(
        r2.is_ok(),
        "impl Foo for Vec<String> must NOT overlap impl Foo for Vec<i32>"
    );
}

#[test]
fn t12_same_generic_args_do_overlap() {
    // Sanity check: two impls with identical generic self types still conflict.
    let local_krate = CrateId::from_raw(0);
    let mut interner = global_interner();
    let def_map = build_def_map(&mut interner, local_krate, &["Foo", "Vec", "String"]);
    let mut ctx = make_ty_ctx();
    let mut infer = glyim_solve::InferenceTable::new();
    let mut checker = CoherenceChecker::new(&def_map);

    let impl_a = make_generic_impl_item(&mut interner, "Foo", "Vec", &["i32"]);
    let impl_b = make_generic_impl_item(&mut interner, "Foo", "Vec", &["i32"]);

    checker
        .check_and_register_impl_compat(
            &impl_item_to_header(&impl_a, &mut interner, &mut ctx, &def_map),
            ImplPolarity::Positive,
            &mut ctx,
            &mut infer,
        )
        .unwrap();

    let r2 = checker.check_and_register_impl_compat(
        &impl_item_to_header(&impl_b, &mut interner, &mut ctx, &def_map),
        ImplPolarity::Positive,
        &mut ctx,
        &mut infer,
    );
    assert!(
        r2.is_err(),
        "two impl Foo for Vec<i32> must conflict (overlap)"
    );
}
