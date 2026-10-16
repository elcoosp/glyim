// ANTI-REGRESSION: two instantiations of the *same* generic function in one
// program must not share an LLVM symbol.
//
// `Option::<i32>::unwrap` and `Option::<Option<i32>>::unwrap` previously
// both mangled to `__glyim_fn_<id>`; the first definition won and the
// second call site hit an undefined symbol at link. The symbol is now
// derived from the `(FnDefId, Substitution)` pair
// (`glyim_type::mangle_fn_symbol`) — see commit "fix(codegen): thread
// mangled FnDef symbol through mono → codegen".
//
// A failure here manifests as a link error: `Undefined symbols:
// ___glyim_fn_<id>` — or, if the mangling were subtly wrong, as silently
// wrong-code (the wrong instantiation's body reached for the caller's
// arguments). Either is a hard test failure.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    // Two instantiations of `Option::<T>::unwrap` at *different* `T`s.
    let a: i32 = Option::Some(7).unwrap();                  // T = i32
    let b: i32 = Option::Some(Option::Some(9)).unwrap().unwrap(); // T = Option<i32>, then i32
    // And a third at a third `T`, to make the collision surface louder if
    // the mangling ever regresses to a hash-of-something-too-narrow.
    let c: i32 = Option::Some(Option::Some(Option::Some(26))).unwrap().unwrap().unwrap();
    a + b + c
}
