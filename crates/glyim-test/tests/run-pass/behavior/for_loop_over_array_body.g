// ANTI-REGRESSION: a nontrivial for-loop body over an array.
//
// Extends `for_loop_over_array.g` with a conditional inside the body
// (the `if` branch must not corrupt the loop index or exit condition)
// and an expression body that reads the binding twice.
// test-mode: run-pass
// exit-code: 15
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = [1, 2, 3, 4, 5, 6];
    let mut s = 0;
    for x in a {
        if x > 3 { s = s + x; }
    }
    // 4+5+6 = 15
    s
}
