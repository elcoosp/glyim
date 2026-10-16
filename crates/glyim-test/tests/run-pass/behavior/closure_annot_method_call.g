// Regression: a closure whose annotated `&str` param has a method called
// on it. Guards both the annotation fix and the resolved-formal seeding.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = |s: &str| s.len() as i32;
    f("hello")
}
