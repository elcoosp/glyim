// ANTI-MISCOMPILATION: matching a `Result` returned by `parse::<T>()`
// must extract the *parsed value* from the correct field, with the
// correct ABI.
//
// The `Expr::MethodCall` arm previously returned the outer (unsubstituted)
// `ret_ty` to the caller's `let`, while the call's destination local was
// the substituted shape. The two disagreed, and a downstream `match`
// tried to lay out the raw `<T as FromStr>::Err` projection. If that
// hadn't ICEd, the payload would have been read from the wrong byte
// offset — a silent wrong value.
//
// Three distinct primes in the `Ok` arm and zero contribution from the
// `Err` arm. Any wrong-field extraction or payload misread makes the sum
// differ from 223.
// test-mode: run-pass
// exit-code: 223
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    let a = match "71".parse::<i32>() {
        Result::Ok(v) => v,               // 71
        Result::Err(_) => -1,             // must NOT be taken
    };
    let b = match "73".parse::<i32>() {
        Result::Ok(v) => v,               // 73
        Result::Err(_) => -1,
    };
    let c = match "79".parse::<i32>() {
        Result::Ok(v) => v,               // 79
        Result::Err(_) => -1,
    };
    // 71 + 73 + 79 = 223
    a + b + c
}
