// Regression: `ref mut` write-through. `bind_pattern` used to store a
// `Move` of a *temp copy* of the field, so `*v = ..` wrote into the copy,
// not the scrutinee. Fixed by threading the original place through and
// emitting `Ref(orig_place, Mut)`.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let mut o = Option::Some(1);
    match o {
        Option::Some(ref mut v) => { *v = 42; }
        Option::None => {}
    }
    o.unwrap()
}
