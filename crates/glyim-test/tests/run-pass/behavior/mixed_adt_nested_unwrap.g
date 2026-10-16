// ANTI-REGRESSION: mixed nested ADT unwrap chains.
//
// `Option<Result<T, E>>` and `Result<Option<T>, E>` exercise the
// `(FnDefId, Substitution)` mangling across *two different* ADTs whose
// instantiation arguments are themselves ADTs. A mangling collision
// between `Option::<Result<i32,i32>>::unwrap` and
// `Result::<Option<i32>,i32>::unwrap` — both of which have a single
// `Substitution` entry of *different* types but identical length — would
// produce a link error or a wrong-ABI call.
// test-mode: run-pass
// exit-code: 17
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a: Option<Result<i32, i32>> = Option::Some(Result::Ok(8));
    let b: Result<Option<i32>, i32> = Result::Ok(Option::Some(9));
    a.unwrap().unwrap() + b.unwrap().unwrap()
}
