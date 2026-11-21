// Strict-xfail witness for GAP-0008 (ledger/gaps/audit/193-findings-audit.toml):
// audit finding FE-11 — a leading `|` on a match arm emits a spurious
// [P0001] `expected pattern, found Or`. `parse_pat` handles `a | b` only
// AFTER the first pattern has been parsed (the `PatOr` loop starts on the
// trailing `|`), so the leading pipe is never consumed and the arm's
// pattern parse fails with a cascading error set. The program is valid
// Glyim; while GAP-0008 is open the compiler must keep rejecting it with
// this loud parse cascade (verified live at HEAD 6bb6d666) rather than
// silently mis-parsing the arm.
//
// The line annotations below pin that cascade strictly (the compile-fail
// corpus grammar, M1.4): every produced diagnostic — including the duplicate
// emission of the same parse errors by the expansion pre-parse and the
// pipeline's own parse — must stay consumed, and nothing else may appear.
// When the parser learns to consume a leading `|`, every error disappears
// and this fixture FAILS with `known-gap GAP-0008 closed` (strict xfail,
// plan §3.3): retire the witness in the same change — migrate the program
// to the pass corpus and flip the GAP-0008 entry to closed.
//@ mode: compile-fail
//@ expect-error: expected pattern
//@ feature: LANG-MATCH-GUARDS, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ known-gap: GAP-0008
fn main() -> i32 {
    let x = 2;
    let r = match x {
        | 1 => 10 //~ ERROR expected pattern, found Or
        //~| expected FatArrow, found IntLit
        //~| expected FatArrow, found IntLit
        //~| expected pattern, found FatArrow
        //~| expected FatArrow, found IntLit
        //~| expected pattern, found Or
        //~| expected FatArrow, found IntLit
        //~| expected pattern, found FatArrow
        | 2 => 20 //~ ERROR expected pattern, found FatArrow
        //~| expected FatArrow, found IntLit
        //~| expected pattern, found FatArrow
        //~| expected FatArrow, found IntLit
        _ => 30
    };
    r - 20
}
