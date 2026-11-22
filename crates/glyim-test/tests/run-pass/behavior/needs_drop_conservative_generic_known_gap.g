// M1.7 strict-xfail witness for GAP-0005 (plan §3.3 / ledger
// `ledger/gaps/known/0002-known-gaps-migration.toml`).
//
// GAP-0005 — "`TyCtx::needs_drop` is conservative for `dyn Trait` /
// generic / projection types (drop glue may be skipped)" — caps
// `COMPILER-LAYOUT-SIZE-ALIGN` at L2. The conservative site is the
// `_ => false` catch-all in `TyCtx::needs_drop_rec`
// (`crates/glyim-type/src/ty_ctx.rs`): every `TyKind` not explicitly
// handled — `Param` (a generic type parameter), `Dynamic` (`dyn Trait`),
// `Projection` — returns `false`, so the compiler concludes "no drop
// glue needed" even when the concrete type carries a destructor.
//
// This witness pins the GENERIC-PARAM manifestation, which is the one the
// pipeline architecture guarantees is observable: drop-glue placement is
// decided in the LOWERING (`glyim-lower/src/builder.rs`, the scope-end
// `needs_drop(decl.ty)` loop) on the *generic* MIR, where the local's type
// is `Param(T)`. Monomorphization (`glyim-lower/src/mono.rs`) then
// SUBSTITUTES concrete types into the already-built MIR — it does not
// re-lower or re-evaluate `needs_drop` — so the drop glue that was absent
// at lowering stays absent after `T = Guard` is substituted. `Guard::drop`
// is therefore skipped when the generic local `y` goes out of scope.
//
// While GAP-0005 is open, `hold::<Guard>` returns without dropping `y`, so
// `DROPS` stays 0 and `main` returns `0`. The `//@ known-gap:` header joins
// this fixture to the registry entry. When `needs_drop` stops returning a
// blanket `false` for `Param` (by deferring the decision past
// monomorphization, or by conservatively returning `true` for type
// parameters), drop glue is emitted for `y`, `Guard::drop` runs, `DROPS`
// becomes 1, and the exit code becomes 1 — the fixture FAILS with
// `known-gap GAP-0005 closed` (strict xfail, plan §3.3), forcing the
// witness to be retired in the same change.
//
// The exit code (not stdout) pins the behaviour on purpose, matching the
// GAP-0004 witness: the run-mode stdout matcher is line-anchored
// containment, so a destructor that ran but printed nothing would not be
// caught; the exact exit-code check is.
//
// RISK (recorded in the PR body): the witness's flip depends on the
// lower/mono evaluation order documented above (lower bakes the
// `needs_drop` result; mono substitutes without re-evaluating). If a
// future pipeline change re-lowers per instance, `needs_drop(Guard)` would
// return `true` post-mono and the fixture would fail as "gap closed"
// prematurely — at which point the witness retires alongside the gap.
//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-LAYOUT-SIZE-ALIGN, COMPILER-BCK-DROP-FLAGS, STD-CORE-OPS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ known-gap: GAP-0005
static mut DROPS: i32 = 0;

struct Guard { _marker: i32 }

impl Drop for Guard {
    fn drop(&mut self) { unsafe { DROPS = DROPS + 1; } }
}

// Generic local, not a parameter: `y: T` is a body-local whose drop glue
// is decided at lowering time against `Param(T)` (needs_drop => false).
// `x` (the parameter) is moved into `y`, so the parameter slot itself is
// never dropped here; `y` is the local whose destructor must run.
fn hold<T>(x: T) {
    let _y: T = x;
}

fn main() -> i32 {
    hold(Guard { _marker: 0 });
    // `Guard::drop` was skipped (needs_drop(Param T) = false at lowering,
    // mono does not re-evaluate), so `DROPS` is still 0.
    let observed: i32 = unsafe { DROPS };
    observed
}
