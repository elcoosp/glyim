// test-mode: ui
// compile-flags: --with-stdlib
//
// `Result::Ok(5).unwrap()` leaves the `E` parameter of `Result` with
// nothing in the program to pin it. Emit rustc's E0282 diagnostic
// instead of letting the unbound var reach codegen (where the MIR
// aggregate would carry `[i32, ERROR]` and the LLVM layout pass ICEd
// with `UnknownType(Ty(0))`).
fn main() -> i32 {
    Result::Ok(5).unwrap()
}
