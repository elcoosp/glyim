// M1.7 strict-xfail witness for GAP-0004 (plan §3.3).
//
// A partial move out of a value whose type implements `Drop` currently
// disables the parent destructor *entirely* (KNOWN_GAPS §6: sound — it
// over-retains and never double-frees — but the still-initialized
// siblings leak their cleanup). While GAP-0004 is open, `Pair::drop`
// must NOT run after `p.a` is moved out, so the `DROPS` counter stays 0
// and the program exits `0 + moved = 1`.
//
// The `//@ known-gap:` header joins this fixture to
// `ledger/gaps/known/0002-known-gaps-migration.toml`; the M1.7 run-mode
// inversion applies the strict-xfail contract to run fixtures too. When
// fine-grained partial-move drop guards land, `Pair::drop` runs, the
// counter reads 1, and the exit code becomes 2 — the fixture FAILS with
// `known-gap GAP-0004 closed`, forcing the witness to be retired in the
// same change.
//
// The exit code (not stdout) pins the behaviour on purpose: the run-mode
// stdout matcher is line-anchored containment, so an *added* destructor
// print would not be caught; the exact exit-code check is.
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: COMPILER-BCK-DROP-FLAGS, COMPILER-BCK-MOVE-ANALYSIS, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
//@ known-gap: GAP-0004
static mut DROPS: i32 = 0;

struct Item { v: i32 }
struct Pair { a: Item, b: Item }

impl Drop for Pair {
    fn drop(&mut self) { unsafe { DROPS = DROPS + 1; } }
}

fn take(it: Item) -> i32 { it.v }

fn main() -> i32 {
    let p = Pair { a: Item { v: 1 }, b: Item { v: 2 } };
    let moved = take(p.a);
    unsafe { DROPS + moved }
}
