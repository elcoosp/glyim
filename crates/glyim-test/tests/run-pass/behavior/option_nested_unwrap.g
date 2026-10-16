// Nested `Option<Option<i32>>` unwrap chain.
//
// Regression for two separate bugs that both surfaced as
// `UnknownType(Ty(16))` / an undefined `__glyim_fn_362` at link:
//
// 1. Mono enqueued the *raw* generic body of `Option::<T>::unwrap`
//    alongside its substituted instantiation. The raw body was filtered
//    out (see `body_is_raw_generic` in glyim-pipeline).
// 2. Both instantiations of `Option::<T>::unwrap` mangled to the same
//    plain `__glyim_fn_362`. The symbol is now derived from the
//    `(FnDefId, Substitution)` pair (`glyim_type::mangle_fn_symbol`),
//    and the call site's substitution is resolved through the inference
//    table before the mangling key is computed (so the *inner* `.unwrap`
//    on an `Infer(TyVar)` receiver still produces the concrete key).
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let o: Option<Option<i32>> = Option::Some(Option::Some(6));
    o.unwrap().unwrap()
}
