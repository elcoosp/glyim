// Same impl, different method: exercises the candidate loop's non-matching
// iterations. `unwrap_or_else` on `None` invokes the closure → 11.
// test-mode: run-pass
// exit-code: 11
// compile-flags: --with-stdlib
fn main() -> i32 {
    let o: Option<i32> = Option::None;
    o.unwrap_or_else(|| 11)
}
