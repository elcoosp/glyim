// ANTI-REGRESSION: nested `Result<Result<i32,i32>,i32>` unwrap chain.
//
// Same three-layer stack as `option_nested_unwrap.g` (raw-generic leak,
// symbol mangling, infer-table receiver resolution), but at a different
// ADT and with *two* generic parameters on the outer `Result`. This
// exercises the `(FnDefId, Substitution)` mangling key with a non-unit
// second parameter, which is where a hash-by-length-only scheme would
// collide.
// test-mode: run-pass
// exit-code: 7
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let r: Result<Result<i32, i32>, i32> = Result::Ok(Result::Ok(7));
    r.unwrap().unwrap()
}
