// Regression: `Option::map` with a closure. Two compounding causes:
// callee FnDef substs missing method generics; closure arg unseeded.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let o = Option::Some(5);
    let r = o.map(|x| x + 1);
    r.unwrap()
}
