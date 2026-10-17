// ANTI-REGRESSION: a trait whose only method has a default body, with
// a bare `impl T for P { }` (no methods overridden at all).
// test-mode: run-pass
// exit-code: 100
// compile-flags: --with-stdlib
// needs-llvm
trait T { fn m(&self) -> i32 { 100 } }
struct P { n: i32 }
impl T for P { }
fn main() -> i32 { let p = P { n: 5 }; p.m() }
