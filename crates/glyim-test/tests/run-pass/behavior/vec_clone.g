// Regression for KNOWN_GAPS §3.5.4 / P1-C1 second half: `Vec<T>::clone`
// for non-Copy `T` was a compiler-gap diagnostic. It now lowers to
// "extract ptr+len from the Vec's header, memcpy into a fresh buffer"
// via the shared `glyim_slice_to_vec` runtime helper. The clone must be
// independent — mutating the original after cloning must not affect the
// clone's length.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    // Basic clone preserves length.
    let v: Vec<i32> = [1, 2, 3].to_vec();
    let w = v.clone();
    let basic = v.len() == 3 && w.len() == 3;

    // Independent: mutating the original after clone must not touch the clone.
    let mut a: Vec<i32> = [10, 20, 30].to_vec();
    let b = a.clone();
    a.push(40);
    let independent = a.len() == 4 && b.len() == 3;

    if basic && independent { 0 } else { 1 }
}
