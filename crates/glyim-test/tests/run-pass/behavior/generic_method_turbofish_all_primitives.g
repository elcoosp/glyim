// ANTI-REGRESSION: `"X".parse::<T>()` for every primitive `FromStr` impl
// must compile *and run* without a segfault.
//
// The turbofish `::<T>` names the method's own generic parameters
// (`str::parse<T>`), not the impl's. It was previously read as the
// *impl*-level substitution, so the callee got `FnDef(id, [T, <fresh T>])`
// — arity 2 instead of 1 — and its substituted return type stayed
// `Result<Param(0), _>`. The caller's destination local was
// `Result<Infer, _>` (→ `Ty::ERROR` after zonk), the layouts disagreed,
// and the codegen emitted a store through a null pointer. The program
// built fine but SIGSEGV'd at runtime.
//
// See commit "fix(typeck): place method turbofish in method's own
// generics, not impl's".
//
// A failure here is a runtime SIGSEGV (exit 139 / signal 11) with no
// diagnostic — the exact reason existing tests passed: they only checked
// *compilation*, not execution.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
// needs-llvm
fn main() {
    // Every primitive `FromStr` impl, so a regression at any one of them
    // surfaces as a distinct runtime failure.
    let _ = "255".parse::<u8>();
    let _ = "65535".parse::<u16>();
    let _ = "4294967295".parse::<u32>();
    let _ = "18446744073709551615".parse::<u64>();
    let _ = "42".parse::<usize>();
    let _ = "-128".parse::<i8>();
    let _ = "-32768".parse::<i16>();
    let _ = "-2147483648".parse::<i32>();
    let _ = "-9223372036854775808".parse::<i64>();
    let _ = "-42".parse::<isize>();
    let _ = "3.14".parse::<f64>();
}
