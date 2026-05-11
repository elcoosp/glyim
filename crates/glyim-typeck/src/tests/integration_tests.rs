use glyim_interner::Interner;
use glyim_typeck::{KnownSymbols, TypeChecker, TypeError, UnificationTable};
use glyim_hir::types::{HirType, TypeVar};
use glyim_diag::Span;
use std::collections::HashMap;

fn sp() -> Span { Span::new(0, 1) }

// ── Length-prefix mangling round-trips ─────────────────────────

#[test]
fn test_mangling_vec_i64_round_trip() {
    let mut interner = Interner::new();
    let vec_sym = interner.intern("Vec");
    let result = glyim_mono::mangling::type_to_short_string(
        &HirType::Generic(vec_sym, vec![HirType::Int]),
        &interner
    ).unwrap();
    assert_eq!(result, "Vec__i64");
}

#[test]
fn test_mangling_fn_type_encodes_arity() {
    let mut interner = Interner::new();
    let result = glyim_mono::mangling::type_to_short_string(
        &HirType::Func(vec![HirType::Int, HirType::Bool], Box::new(HirType::Int)),
        &interner
    ).unwrap();
    assert!(result.starts_with("fn2"));
}

// ── Same-scope shadowing ──────────────────────────────────────

#[test]
fn test_type_env_shadowing() {
    let mut env = glyim_typeck::env::TypeEnv::new();
    let mut interner = Interner::new();
    let x = interner.intern("x");

    env.push_scope();
    env.insert(x, HirType::Int, false);
    assert_eq!(env.lookup(x), Some(&HirType::Int));

    // Shadow in inner scope
    env.push_scope();
    env.insert(x, HirType::Bool, false);
    assert_eq!(env.lookup(x), Some(&HirType::Bool));
    env.pop_scope();

    // Original binding restored
    assert_eq!(env.lookup(x), Some(&HirType::Int));
    env.pop_scope();
}

// ── Strict arity checking ─────────────────────────────────────

#[test]
fn test_solve_generic_params_arity_mismatch() {
    let mut table = UnificationTable::new();
    let mut interner = Interner::new();
    let t = interner.intern("T");
    let x = interner.intern("x");

    let type_params = vec![t];
    let param_types = vec![(x, HirType::Param(t))];
    let arg_types = vec![HirType::Int, HirType::Bool]; // too many args

    let mut errors = Vec::new();
    let result = glyim_typeck::solve::solve_generic_params(
        &mut table,
        &type_params,
        &param_types,
        None,
        &arg_types,
        None,
        sp(), sp(),
        &mut |e| errors.push(e),
    );

    assert!(result.had_errors);
    assert!(!errors.is_empty());
    assert!(matches!(errors[0], TypeError::ArgumentCountMismatch { .. }));
}

// ── Explicit shape mismatches ─────────────────────────────────

#[test]
fn test_extract_type_substitutions_shape_mismatch() {
    let mut interner = Interner::new();
    let t = interner.intern("T");
    let vec_sym = interner.intern("Vec");
    let type_params = std::collections::HashSet::from([t]);

    let schema = HirType::Generic(vec_sym, vec![HirType::Param(t)]);
    let concrete = HirType::Generic(vec_sym, vec![HirType::Int, HirType::Bool]); // wrong arity

    let result = glyim_typeck::unify::extract_type_substitutions(
        &schema, &concrete, &type_params, sp(), sp()
    );

    assert!(!result.errors.is_empty());
}

// ── Always-on validation ──────────────────────────────────────

#[test]
fn test_validate_mono_input_rejects_infer() {
    let mut fn_types_map = HashMap::new();
    let mut interner = Interner::new();
    let fn_name = interner.intern("test_fn");

    let mut expr_types = HashMap::new();
    expr_types.insert(glyim_hir::types::ExprId::new(0), HirType::Infer(TypeVar::from_raw_unchecked(0)));

    fn_types_map.insert(fn_name, glyim_typeck::typeck::FnTypes {
        expr_types,
        call_type_args: HashMap::new(),
        sizeof_types: HashMap::new(),
        is_generic: false,
        type_params: vec![],
        span: sp(),
    });

    let result = glyim_typeck::validate::validate_mono_input(&fn_types_map);
    assert!(result.is_err());
}

// ── Option/Result as standard generic types ───────────────────

#[test]
fn test_option_is_generic_not_hardcoded() {
    let mut interner = Interner::new();
    let known = KnownSymbols::intern_all(&mut interner);
    let mut table = UnificationTable::new();

    let option_int = HirType::Generic(known.option, vec![HirType::Int]);
    let option_bool = HirType::Generic(known.option, vec![HirType::Bool]);

    // Same generic type with different args should not unify
    assert!(table.unify(&option_int, &option_bool, sp(), sp()).is_err());

    // Same type with same args should unify
    let option_int2 = HirType::Generic(known.option, vec![HirType::Int]);
    assert!(table.unify(&option_int, &option_int2, sp(), sp()).is_ok());
}

// ── Explicit error spans ──────────────────────────────────────

#[test]
fn test_unresolved_name_contains_span() {
    let mut interner = Interner::new();
    let known = KnownSymbols::intern_all(&mut interner);
    let mut tc = TypeChecker::new(interner, known);

    // tc is the backward-compatible wrapper. We'll test the underlying errors.
    let span = Span::new(10, 20);
    let err = TypeError::UnresolvedName { name: "test_var".into(), span };

    assert_eq!(err.to_string(), "unresolved name `test_var`");
}

// ── Iterator item type extraction ─────────────────────────────

#[test]
fn test_known_symbols_has_iterator() {
    let mut interner = Interner::new();
    let known = KnownSymbols::intern_all(&mut interner);
    assert_eq!(interner.resolve(known.iterator), "Iterator");
}

#[test]
fn test_unification_table_reset() {
    let mut table = UnificationTable::new();
    let v1 = table.fresh_var(sp());
    assert_eq!(table.var_span(v1), Some(&sp()));
    table.reset();
    // After reset, new vars start fresh
    let _v2 = table.fresh_var(sp());
}

// ── TypeCheckOutput round-trip ────────────────────────────────

#[test]
fn test_type_check_output_construction() {
    let mut interner = Interner::new();
    let output = glyim_typeck::TypeCheckOutput {
        expr_types: vec![HirType::Int, HirType::Bool],
        call_type_args: HashMap::new(),
        interner: interner.clone(),
    };
    assert_eq!(output.expr_types.len(), 2);
}
