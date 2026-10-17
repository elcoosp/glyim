// ANTI-REGRESSION / ANTI-MISCOMPILATION: casting the result of a generic
// method whose return type is an *impl-level* generic parameter must
// work.
//
// `impl<T> Option<T> { fn unwrap(self) -> T }` has an `Infer(TyVar)` as
// the sig output when `resolve_method_call` instantiates the impl's
// param map. That var is bound during the receiver's ADT unify
// (`Option<u8>` pins it to `u8`), but `subst_ty` only walks `Param(i)`
// — it never looks at the inference table. So `ret_ty` stayed
// `Infer(TyVar)`, and a downstream `as i64` on it produced `invalid
// cast` because `is_valid_cast` has no `Infer(Ty)` source arm (only
// `Infer(Int)` / `Infer(Float)`).
//
// The fix resolves `ret_ty` through the inference table after
// `subst_ty`, folding the bound var to its concrete value before the
// cast check runs.
//
// The fixture uses distinct primes so a regression that produced a
// wrong value (e.g. via a stale `Infer` that zonk folded to `ERROR`
// and codegen happened to lay out as zero) returns a different exit
// code — 140 — instead of 140 by accident.
// test-mode: run-pass
// exit-code: 140
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    // Option<u8> unwrap → u8 → i32
    let a: Option<u8> = Option::Some(23);
    let a_i32 = a.unwrap() as i32;                              // 23

    // parse::<u8> unwrap → u8 → i32
    let b = "29".parse::<u8>().unwrap() as i32;                 // 29

    // parse::<i32> unwrap → i32 → i64 → i32
    let c = "37".parse::<i32>().unwrap() as i64 as i32;         // 37

    // annotated Result<i32,i32> unwrap → i32 → i64 → i32
    let r: Result<i32, i32> = Result::Ok(51);
    let d = r.unwrap() as i64 as i32;                           // 51

    // 23 + 29 + 37 + 51 = 140
    a_i32 + b + c + d
}
