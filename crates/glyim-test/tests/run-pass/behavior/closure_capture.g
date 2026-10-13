// ignore
// LLVM backend: Layout error (UnknownType) on closure sig.
// Interp: exit 101 on parametered closures. See docs/design/closures.md.
// test-mode: run-pass
// exit-code: 6
// compile-flags: --with-stdlib
fn main() -> i32 {
    let n = 1;
    let f = |x: i32| -> i32 { x + n };
    f(5)
}
