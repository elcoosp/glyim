// A `static`'s initializer must be const-evaluable (Rust E0015).
// Previously this silently miscompiled to 0; it must be a compile error.
//@ mode: ui
//@ feature: LANG-SCOPE-STATIC-ITEM, LANG-SYN-CONST-ITEM, COMPILER-INTERP-CONST-EVAL, TOOL-TEST-UI
//@ backends: gam
fn compute() -> i32 { 42 }
static X: i32 = compute();
fn main() -> i32 { X }
