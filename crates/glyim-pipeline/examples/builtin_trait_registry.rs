//! Consumer walkthrough for the builtin trait registry.
//!
//! Shows the solver-bootstrap surface the registry now drives from the
//! outside: build a `TraitContext`, run
//! `glyim_pipeline::register_builtin_traits` over it (the same call the
//! pipeline makes before proving any bound), and inspect the table through
//! the registry itself. The `Default`/`Any` tail is the regression this
//! seam fixed — those two used to be registered in the resolver only, so
//! the solver saw an unknown trait id for them.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-pipeline --example builtin_trait_registry
//! ```

use glyim_core::def_id::TraitDefId;
use glyim_core::interner::Interner;
use glyim_solve::solver::TraitContext;
use glyim_type::builtin_traits::BuiltinTraitDef;

fn main() {
    println!(
        "builtin trait registry ({} entries, reserved ids 2001..3000):",
        BuiltinTraitDef::ALL.len()
    );
    for t in BuiltinTraitDef::ALL {
        println!(
            "  {:>4}  {:<12} lang-trait: {}",
            t.trait_id(),
            t.name(),
            t.is_lang_trait(),
        );
    }

    // Solver bootstrap: every registry entry lands in the `TraitContext`
    // under its reserved id, so solver-facing rendering can name any of
    // them. Name canonicalization goes through the registry, not through a
    // second hand-copied table.
    let interner = Interner::new();
    let mut trait_ctx = TraitContext::new();
    glyim_pipeline::register_builtin_traits(&mut trait_ctx, &interner);

    println!("\nsolver bootstrap parity check:");
    for name in ["Default", "Any", "Copy"] {
        let t = BuiltinTraitDef::from_name(name).expect("registry name must resolve");
        let solver_name = trait_ctx
            .trait_name(TraitDefId::from_raw(t.trait_id()))
            .map(|n| interner.resolve(n));
        println!(
            "  {name:<8} id {:>4}  lang-trait: {:<5}  solver registers it as {solver_name:?}",
            t.trait_id(),
            t.is_lang_trait(),
        );
    }
}
