// Strict-xfail witness for GAP-0008 (ledger/gaps/audit/193-findings-audit.toml):
// audit finding FE-11 — a leading `|` on a match arm emits a spurious
// [P0001] `expected pattern, found Or`. `parse_pat` handles `a | b` only
// AFTER the first pattern has been parsed (the `PatOr` loop starts on the
// trailing `|`), so the leading pipe is never consumed and the arm's
// pattern parse fails with cascading errors. The program is valid Glyim;
// while GAP-0008 is open the compiler must keep rejecting it with the
// loud [P0001] diagnostic (verified live at HEAD 6bb6d666) rather than
// silently mis-parsing the arm.
//
// When the parser learns to consume a leading `|`, this fixture compiles
// cleanly and FAILS with `known-gap GAP-0008 closed` (strict xfail, plan
// §3.3): retire the witness in the same change — migrate the program to
// the pass corpus and flip the GAP-0008 entry to closed.
//@ mode: compile-fail
//@ expect-error: expected pattern
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ known-gap: GAP-0008
fn main() -> i32 {
    let x = 2;
    let r = match x {
        | 1 => 10
        | 2 => 20
        _ => 30
    };
    r - 20
}
