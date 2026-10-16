// Regression: `ref` patterns. `Pat::Binding` had no way to express `ref`,
// so the keyword was dropped at HIR lowering, `v` was typed `T`, and
// `*v` failed with "cannot dereference non-pointer type". Fixed by adding
// a `Pat::RefBinding` variant that typeck binds as `&T`/`&mut T`.
// test-mode: run-pass
// exit-code: 5
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let o = Option::Some(5);
    match o {
        Option::Some(ref v) => *v,
        Option::None => 0,
    }
}
