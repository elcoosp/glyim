// ANTI-REGRESSION / ANTI-MISCOMPILATION: struct-update syntax must copy
// the *un-overridden* fields from the base struct, not fill them with
// zero-sized `Error` placeholders.
//
// `lower_rvalue`'s `thir::ExprKind::Struct` arm used to destructure the
// spread as `spread: _` (silently discarded). The aggregate was built
// with only the explicit fields (`P { x: 10, ..p }` → `[10]`); any
// downstream `q.y` read a `Field(1)` out of a one-element aggregate and
// got the layout's `Error` placeholder — the miscompiled program built
// and ran, silently returning `x`'s value instead of `y`'s.
//
// The fix materialises the spread source into a temporary local and
// projects every non-overridden declaration-order field out of it, which
// is what `..p` desugars to. Distinct primes in every field ensure a
// residual placeholder or a swapped field produces a distinct exit
// code.
// test-mode: run-pass
// exit-code: 100
// compile-flags: --with-stdlib
// needs-llvm
struct P { a: i32, b: i32, c: i32 }
fn main() -> i32 {
    let p = P { a: 11, b: 23, c: 37 };
    // Override only `a`; `b` and `c` must come from `p`.
    let q = P { a: 29, ..p };
    // 29 + 23 + 37 = 89
    q.a + q.b + q.c
}
