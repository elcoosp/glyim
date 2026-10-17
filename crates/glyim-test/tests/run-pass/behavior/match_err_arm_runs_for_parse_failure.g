// ANTI-MISCOMPILATION: a runtime parse failure must take the `Err` arm
// and produce the *Err-arm* value, not the `Ok` arm with garbage.
//
// The string `"not_a_number"` cannot be parsed as `i32`, so `from_str`
// returns `Err(ParseIntError)` at runtime. If the compiler mis-lowers
// the discriminant check (or the payload extraction returns the wrong
// variant's bytes), the program may take the `Ok` arm and return
// zero-initialized garbage, or fall through with the wrong exit code.
//
// The `Err` arm's value (97) is deliberately *distinct* from the `Ok`
// arm's (0) so taking the wrong branch produces a different exit code.
// test-mode: run-pass
// exit-code: 97
// compile-flags: --with-stdlib
// needs-llvm
fn main() -> i32 {
    match "not_a_number".parse::<i32>() {
        Result::Ok(_) => 0,               // must NOT be taken
        Result::Err(_) => 97,             // taken at runtime
    }
}
