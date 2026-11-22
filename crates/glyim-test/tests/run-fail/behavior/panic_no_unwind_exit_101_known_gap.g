// M1.7 strict-xfail witness for GAP-0003 (plan §3.3 / ledger
// `ledger/gaps/known/0002-known-gaps-migration.toml`).
//
// GAP-0003 — "`panic!` does not unwind: `glyim_panic` exits 101 without
// running destructors (intentional, documented)" — caps
// `LANG-UNWIND-CLEANUP` at L2 from the RUNTIME side. It is the
// sibling of GAP-0002 (which pins the LOWERING side: no cleanup edges are
// emitted). The two gaps share an observable — a panic exits 101 and
// destructors do not run — but pin DIFFERENT root causes and code paths:
// GAP-0002's witness panics in `main` itself (single frame, the
// lowering-has-no-cleanup-edges case); this witness panics inside a
// CALLEE (`boom`) and threads the panic back through an intermediate
// frame (`outer`) that owns a `Guard`. That exercises the runtime's
// panic-PROPAGATION path (`glyim_panic`'s abort contract across call
// frames), not just the single-frame lowering case.
//
// While GAP-0003 is open, `glyim_panic` aborts with exit 101 directly
// (no unwind), so `outer`'s `_g` is never dropped and `DROPS` stays 0.
// `main` reads the counter into a local BEFORE the panicking call and
// prints `DROPS=0`; the exit code is `101`.
//
// The `//@ known-gap:` header joins this fixture to the registry entry.
// When the runtime implements unwinding (GAP-0003 closed) AND the lowering
// emits cleanup edges (GAP-0002 closed), `Guard::drop` runs on the
// unwinding path through `outer`, `DROPS` becomes 1, the `DROPS=0` stdout
// assertion fails — the fixture FAILS with `known-gap GAP-0003 closed`
// (strict xfail, plan §3.3), forcing the witness to be retired in the
// same change. (Closing GAP-0003 alone — runtime unwinding — without
// GAP-0002's cleanup edges would still leave `DROPS=0`; the witness is
// scoped to GAP-0003 because the runtime-abort contract is the behaviour
// it pins, and the cross-gap closure order is recorded in the ledger.)
//
// The stdout matcher is line-anchored containment; the exit-code check is
// exact. The stdout assertion (`DROPS=0`) is the sharper pin (an added
// destructor that ran but printed nothing would still flip `DROPS`); the
// exit code (101) is the structural pin on `glyim_panic`'s abort contract.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stdout: DROPS=0
//@ compile-flags: --with-stdlib
//@ feature: LANG-UNWIND-CLEANUP, STD-CORE-OPS, STD-CORE-PANIC, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
//@ known-gap: GAP-0003
static mut DROPS: i32 = 0;

struct Guard {}

impl Drop for Guard {
    fn drop(&mut self) { unsafe { DROPS = DROPS + 1; } }
}

// Callee that panics. The panic must propagate back through `outer`'s
// frame (where `_g` lives) to `main`; with no unwind, `Guard::drop` is
// never reached.
fn boom() -> i32 { panic!("boom") }

fn outer() -> i32 {
    let _g = Guard {};
    boom()
}

fn main() -> i32 {
    // Read the counter into a local BEFORE the panicking call so the
    // printed value reflects the pre-panic state (`DROPS=0`).
    let observed: i32 = unsafe { DROPS };
    println!("DROPS={}", observed);
    outer();
    0
}
