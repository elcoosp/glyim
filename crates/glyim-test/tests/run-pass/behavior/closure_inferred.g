// ignore
// LLVM backend: Layout error (UnknownType) on the closure sig.
// Interp: exit 101 on parametered closures. See docs/design/closures.md.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let f = |x: i32| x + 1;
    f(5)
}
