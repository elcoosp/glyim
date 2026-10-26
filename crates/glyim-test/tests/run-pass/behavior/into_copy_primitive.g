// `x.into()` on a Copy primitive with a same-type target must yield the
// same value (P1-C1 tail). Before this fix the compiler resolved
// `into` to the stdlib blanket impl `impl<T> Into<T> for T` (FnDefId
// 477, convert.g), whose body was never collected into the mono set —
// LLVM emitted a `declare` with no `define` and the link failed with
// `Undefined symbols: ___glyim_fn_477`. After the fix, `into` on a
// direct Copy receiver resolves to the sentinel with the receiver type
// as output, and lowering turns it into a plain operand use.
// test-mode: run-pass
// exit-code: 42
// compile-flags: --with-stdlib
fn main() -> i32 {
    let x: i32 = 42;
    let y: i32 = x.into();
    y
}
