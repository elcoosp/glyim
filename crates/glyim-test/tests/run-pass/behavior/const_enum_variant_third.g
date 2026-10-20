// Third-of-three variant, so the emitted discriminant must be exactly 2
// (not just "non-zero").
// test-mode: run-pass
// exit-code: 3
// compile-flags: --with-stdlib
enum C { A, B, D }
const K: C = C::D;
fn main() -> i32 { match K { C::A => 1, C::B => 2, C::D => 3 } }
