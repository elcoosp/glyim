// §3.5.33 companion: mixed divergent-SECOND arm. Scrutinee is Some(5),
// so the value arm (first) fires and exits 0. The divergent arm
// (None => return 99, second) is still type-checked, exercising the fix.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    match Option::Some(5) {
        Option::Some(_) => 0,
        Option::None => return 99,
    }
}
