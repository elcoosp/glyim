// A `for` loop over a `Range<usize>`. Regression coverage for two fixes:
// (1) the for-loop `SwitchInt` must switch on the `Option`'s *discriminant*,
//     not the whole aggregate; (2) `Range<usize>` is itself an `Iterator`.
// test-mode: run-pass
// exit-code: 10
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut n = 0usize;
    for i in 0usize..5usize { n = n + i; }
    n as i32
}
