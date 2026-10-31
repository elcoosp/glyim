// §3.5.33 companion: mixed arms, divergent-FIRST. The scrutinee is a
// typed local `Option::None`, so the value arm (None => 0, second) fires
// at runtime and exits 0; the divergent arm (Some(_) => return 1, first)
// is still type-checked, exercising the all_arms_never tracking.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let r: Option<i32> = Option::None;
    match r {
        Option::Some(_) => return 1,
        Option::None => 0,
    }
}
