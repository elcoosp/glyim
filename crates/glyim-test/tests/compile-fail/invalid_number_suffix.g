// An invalid numeric suffix (the literal `5abc` below) must produce the
// registered lexer code E0001 — the CT-5 negative fixture for the code
// (plan §6 M3.3): the `glyim --explain E0001` surface and the ledger's
// `errors` cross-check both lean on this fixture.
//
// The expectation is declared twice with the continuation form because the
// harness legitimately surfaces the lexer finding once per pipeline entry
// point (the initial parse and the pipeline's own re-parse of the same
// source both reject the suffix, and the harness does not dedup them).
//@ mode: compile-fail
//@ feature: COMPILER-FRONT-LEX, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ expect-error: invalid number suffix
//@ expect-error-code: E0001
fn main() {
    let x = 5abc; //~ ERROR invalid number suffix
    //~| ERROR invalid number suffix
}
