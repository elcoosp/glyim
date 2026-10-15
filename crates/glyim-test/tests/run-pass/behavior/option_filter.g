// KNOWN FAILING — `Option::filter` is not defined in option.g.
// `o.filter(|x| *x > 0)` reports `no method filter found for type`.
// Tracked as a stdlib gap, not a compiler bug.
// test-mode: run-pass
// ignore
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let o = Option::Some(5);
    o.filter(|x| *x > 0).unwrap()
}
