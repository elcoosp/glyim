// A match over an enum that omits a variant must be rejected with the
// registered T0050 non-exhaustive-match code — the CT-5 negative fixture
// for the code (plan §6 M3.3): the `glyim --explain T0050` surface and the
// ledger's `errors` cross-check both lean on this fixture.
//@ mode: compile-fail
//@ feature: LANG-SYN-ENUM, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
//@ expect-error-code: T0050
enum E { A, B }

fn pick(e: E) -> i32 {
    match e { //~~ ERROR non-exhaustive match: missing variants `B`
        E::A => 1,
    }
}

fn main() -> i32 {
    pick(E::A)
}
