//! Builtin-trait-registry cost harness.
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-test`
//! evidence/annotations bench style. Measures the operations the registry
//! introduced as the single source of truth for the compiler-known traits,
//! plus the real consumer path they now drive:
//!
//! 1. `from_name` canonicalization — the path a `.g` bound (`T: Clone`)
//!    takes from source text to reserved id;
//! 2. `from_trait_id` over the full reserved `2001..3000` window — the
//!    lookup diagnostics use to render a solver-side trait id, where the
//!    17 registry hits must be told apart from 982 misses;
//! 3. `register_all` fan-out — the cost of the one registration path both
//!    consumers (resolver bootstrap, solver bootstrap) now share;
//! 4. the full resolver-side bootstrap (`test_ty_ctx().freeze()`), whose
//!    builtin-trait slice is `register_all`-driven since the registry
//!    replaced the hand-copied table in `TyCtxMut::register_builtin_ranges`.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-type --bench builtin_trait_registry
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_type::builtin_traits::{BuiltinTraitDef, BuiltinTraitSink};

/// Collecting sink: mimics what a real consumer does per entry (store the
/// id, keep the name for interning) without needing a context to mutate.
#[derive(Default)]
struct CollectingSink {
    registered: Vec<(u32, &'static str)>,
}

impl BuiltinTraitSink for CollectingSink {
    fn register_builtin_trait(&mut self, trait_id: u32, name: &'static str) {
        self.registered.push((trait_id, name));
    }
}

/// Rounds per measured phase. The registry is tiny (17 entries), so the
/// lookup phases run many rounds to get stable totals.
const ROUNDS: u32 = 20_000;

/// Resolver-side bootstrap round-trips. A full `TyCtx` build is far heavier
/// than the registry itself, so this phase runs fewer rounds.
const BOOTSTRAP_ROUNDS: u32 = 200;

fn main() {
    println!(
        "builtin trait registry: {} entries, ids {}..={}",
        BuiltinTraitDef::ALL.len(),
        BuiltinTraitDef::ALL[0].trait_id(),
        BuiltinTraitDef::ALL[BuiltinTraitDef::ALL.len() - 1].trait_id(),
    );

    // Phase 1: from_name canonicalization across the whole table.
    let names: Vec<&str> = BuiltinTraitDef::ALL.iter().map(|t| t.name()).collect();
    let t = Instant::now();
    let mut hits = 0usize;
    for _ in 0..ROUNDS {
        for name in &names {
            if black_box(BuiltinTraitDef::from_name(name)).is_some() {
                hits += 1;
            }
        }
    }
    let from_name = t.elapsed();
    println!(
        "from_name        {from_name:>9.1?} total ({hits} hits, {} lookups)",
        ROUNDS as usize * names.len(),
    );

    // Phase 2: from_trait_id across the full reserved window — hits must
    // resolve, and misses (the rest of 2001..3000) must reject quickly.
    let t = Instant::now();
    let mut resolved = 0usize;
    for _ in 0..ROUNDS {
        for id in 2001_u32..3000 {
            if black_box(BuiltinTraitDef::from_trait_id(id)).is_some() {
                resolved += 1;
            }
        }
    }
    let from_id = t.elapsed();
    println!(
        "from_trait_id    {from_id:>9.1?} total ({resolved} resolved, {} probes)",
        ROUNDS as usize * 999,
    );

    // Phase 3: register_all fan-out into a collecting sink.
    let t = Instant::now();
    let mut entries = 0usize;
    for _ in 0..ROUNDS {
        let mut sink = CollectingSink::default();
        BuiltinTraitDef::register_all(&mut sink);
        entries += black_box(sink.registered.len());
    }
    let register_all = t.elapsed();
    println!(
        "register_all     {register_all:>9.1?} total ({entries} entries delivered)",
    );

    // Phase 4: the real consumer path — resolver-side bootstrap, which
    // drives register_builtin_ranges (ADTs, methods, and now the
    // registry-driven builtin trait slice) and freezes. Trait names are
    // probed through the frozen context's own resolver, exactly like the
    // parity test does.
    let t = Instant::now();
    let mut names_frozen = 0usize;
    for _ in 0..BOOTSTRAP_ROUNDS {
        let ctx = glyim_test::test_ty_ctx().freeze();
        let resolver = ctx.resolver();
        for t in BuiltinTraitDef::ALL {
            if ctx.trait_by_name.contains_key(&resolver.intern(t.name())) {
                names_frozen += 1;
            }
        }
    }
    let bootstrap = t.elapsed();
    println!(
        "ty_ctx bootstrap {bootstrap:>9.1?} total ({BOOTSTRAP_ROUNDS} rounds, {names_frozen} trait names seen)",
    );
}
