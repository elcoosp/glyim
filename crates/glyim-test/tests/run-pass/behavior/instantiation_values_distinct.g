// ANTI-MISCOMPILATION: three nested `Option::<T>::unwrap` instantiations
// with *distinct* payloads. If a symbol-mangling regression makes two
// instantiations share a definition, the caller whose `T` doesn't match
// the definition's `T` reads the wrong field of the wrong layout and
// returns either a tag value (0 or 1) or a misaligned integer.
//
// Nesting is 3 deep so `T` values are `i32`, `Option<i32>`, and
// `Option<Option<i32>>` — a distinct mangling key is required for each.
//
// If the symbol collision were silently resolved by taking the first
// definition for all three, the outer two calls would return the wrong
// value (or crash on stack corruption). The summed exit code (65) is
// distinctive; any substitution that returns a tag or a payload from a
// different layer produces a different total.
// test-mode: run-pass
// exit-code: 65
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: i32 = Option::Some(11).unwrap();                                    // 11
    let b: i32 = Option::Some(Option::Some(23)).unwrap().unwrap();             // 23
    let c: i32 = Option::Some(Option::Some(Option::Some(31)))                  // 31
        .unwrap()
        .unwrap()
        .unwrap();
    // 11 + 23 + 31 = 65
    a + b + c
}
