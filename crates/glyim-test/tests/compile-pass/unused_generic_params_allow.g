// §3.5.30 framework: `-A unused_generics` suppresses the warning.
//@ mode: compile-pass
//@ compile-flags: -A unused_generics
//@ forbid-warning: unused generic parameter
//@ feature: COMPILER-MONO-POLYMORPHIZE, LANG-SCOPE-ATTR-ALLOW, TOOL-TEST-COMPILE-PASS
//@ backends: gam
fn unused<T>() -> i32 {
    42
}

fn main() -> i32 {
    unused::<i32>() - 42
}
