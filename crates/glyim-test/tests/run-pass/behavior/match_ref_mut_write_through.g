// Regression: `ref mut` write-through. `bind_pattern` used to store a
// `Move` of a *temp copy* of the field, so `*v = ..` wrote into the copy,
// not the scrutinee. Fixed by threading the original place through and
// emitting `Ref(orig_place, Mut)`.
//@ mode: run-pass
//@ expect-exit: 42
//@ compile-flags: --with-stdlib
//@ needs-llvm
//@ feature: LANG-MATCH-GUARDS, LANG-TY-REF-MUT, LANG-SYN-MUT, LANG-TY-REF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut o = Option::Some(1);
    match o {
        Option::Some(ref mut v) => { *v = 42; }
        Option::None => {}
    }
    o.unwrap()
}
