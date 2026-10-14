// `for x in &slice` summing element values (7+8+9). Proves the slice
// range advances by `size_of::<i32>()`, not 1 byte.
// needs-llvm
// test-mode: run-pass
// exit-code: 24
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a = [7, 8, 9];
    let s: &[i32] = &a;
    let mut n = 0;
    for x in s { n = n + *x; }
    n
}
