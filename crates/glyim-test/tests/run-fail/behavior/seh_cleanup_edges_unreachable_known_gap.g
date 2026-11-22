// M1.7/M3.3(c) strict-xfail witness for GAP-0002 (plan §3.3 / ledger
// `ledger/gaps/known/0001-known-gaps-seed.toml`).
//
// GAP-0002 — "Windows SEH cleanup edges are unreachable: no lowering
// site sets cleanup (panic exits 101)" — caps `LANG-UNWIND-CLEANUP` at
// L2. The gap is Windows-SEH-shaped in its title, but the *observable*
// behaviour is cross-platform: when the program panics, the panic path
// exits 101 *without* running destructors, because no lowering site
// emits cleanup edges that would unwind the stack and drop locals.
// This fixture pins that observable on Linux: a value whose `Drop` impl
// is in scope when `panic!` fires must NOT have its destructor run, so
// the `DROPS` counter stays `0`. The program reads the counter into a
// local before panicking and prints it as `DROPS=0`; the exit code is
// `101`.
//
// The `//@ known-gap:` header joins this fixture to the registry entry;
// `ledger check` verifies the header names the gap and that the gap
// entry points back here. When cleanup edges land (the lowering emits
// drop glue on the panic path), `Pair::drop` runs, `DROPS` becomes `1`,
// the `DROPS=0` stdout assertion fails — the fixture FAILS with
// `known-gap GAP-0002 closed` (strict xfail, plan §3.3), forcing the
// witness to be retired in the same change (migrate the program to the
// run-pass corpus and flip the ledger gap entry to closed).
//
// The stdout matcher is line-anchored containment; the exit-code check
// is exact. Both together pin the behaviour on purpose: an added
// destructor that ran but printed nothing would leave exit 101 but
// change `DROPS` to `1`, breaking the stdout match — so the stdout
// assertion is the sharper pin and the exit code is the structural one.
//@ mode: run-fail
//@ expect-exit: 101
//@ expect-stdout: DROPS=0
//@ compile-flags: --with-stdlib
//@ feature: LANG-UNWIND-CLEANUP, STD-CORE-OPS, STD-CORE-PANIC, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-FAIL
//@ backends: gam, llvm
//@ known-gap: GAP-0002
static mut DROPS: i32 = 0;

struct Pair { a: i32, b: i32 }

impl Drop for Pair {
    fn drop(&mut self) { unsafe { DROPS = DROPS + 1; } }
}

fn main() -> i32 {
    let _p = Pair { a: 1, b: 2 };
    // The panic path exits 101 without running `_p`'s destructor: no
    // lowering site sets cleanup edges (GAP-0002), so `DROPS` stays 0.
    // Reading the counter into a local before the panic pins the value
    // the run path actually observed; the `println!` then surfaces it
    // for the stdout matcher.
    let observed: i32 = unsafe { DROPS };
    println!("DROPS={}", observed);
    panic!("boom")
}
