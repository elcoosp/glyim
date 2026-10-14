// `Vec::new()` with an explicit element annotation, then `push`/`len`.
// Regression: `Vec::new()`'s registered output is `Vec<Param(0)>`; the old
// `Adt::fn` path substituted the callee ADT's *rigid* `Param(0)` back in,
// leaving the element permanently rigid. It must become a fresh inference
// var so the annotation/`push` can bind it.
// test-mode: run-pass
// exit-code: 2
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    v.len() as i32
}
