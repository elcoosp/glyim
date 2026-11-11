//! Solver-bootstrap parity tests for the builtin trait registry.
//!
//! The pipeline's `register_builtin_traits` used to hand-copy the builtin
//! trait table from `TyCtxMut::register_builtin_ranges`, and the copy had
//! silently drifted: `Default` (2016) and `Any` (2017) were registered in
//! the type context but never reached the solver's `TraitContext`, so
//! solver-facing consumers (trait-name rendering in unsatisfied-bound
//! diagnostics, and any future structural treatment) saw an unknown trait
//! id for them. The registry (`glyim_type::builtin_traits`) is now the
//! single source of truth; these tests pin the solver side to it:
//!
//! * every registry trait is registered in the solver's trait table under
//!   its reserved id and resolvable name;
//! * exactly the five structural traits are additionally registered as
//!   lang traits (structurally discharged bounds);
//! * nothing outside the registry occupies the builtin id range.

use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_solve::solver::TraitContext;
use glyim_type::builtin_traits::BuiltinTraitDef;

use crate::register_builtin_traits;

#[test]
fn the_solver_knows_every_registry_trait_by_name_and_id() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    register_builtin_traits(&mut trait_ctx, &interner);

    for t in BuiltinTraitDef::ALL {
        let id = TraitDefId::from_raw(t.trait_id());
        let name = trait_ctx.trait_name(id).unwrap_or_else(|| {
            panic!(
                "solver bootstrap dropped builtin trait `{}` (id {}) — \
                     the solver's trait table drifted from the shared registry",
                t.name(),
                t.trait_id(),
            )
        });
        assert_eq!(
            interner.resolve(name),
            t.name(),
            "solver registered id {} under a different name than the registry",
            t.trait_id(),
        );
    }
}

#[test]
fn default_and_any_reach_the_solver() {
    // Regression pin for the drift this milestone fixed: these two were
    // registered in `TyCtxMut` only.
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    register_builtin_traits(&mut trait_ctx, &interner);
    for (id, expected) in [(2016_u32, "Default"), (2017, "Any")] {
        let name = trait_ctx.trait_name(TraitDefId::from_raw(id));
        assert_eq!(
            name.map(|n| interner.resolve(n)),
            Some(expected),
            "solver is missing builtin trait {expected} ({id})"
        );
    }
}

#[test]
fn exactly_the_five_structural_traits_are_lang_traits_in_the_solver() {
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    register_builtin_traits(&mut trait_ctx, &interner);

    let lang: Vec<String> = BuiltinTraitDef::ALL
        .iter()
        .filter(|t| t.is_lang_trait())
        .map(|t| t.name().to_string())
        .collect();
    assert_eq!(lang, vec!["Copy", "Sized", "Send", "Sync", "Unpin"]);

    for t in BuiltinTraitDef::ALL {
        let id = TraitDefId::from_raw(t.trait_id());
        let is_lang = trait_ctx.builtin_trait_kind(id).is_some();
        assert_eq!(
            is_lang,
            t.is_lang_trait(),
            "`{}` lang-trait status disagrees between the solver and the \
             registry (registered as lang trait: {is_lang}, registry says: {})",
            t.name(),
            t.is_lang_trait(),
        );
    }
}
