// §3.5.30 framework: `-D unused_generics` promotes the warning to error.
//@ mode: compile-fail
//@ compile-flags: -D unused_generics
//@ feature: COMPILER-MONO-POLYMORPHIZE, TOOL-TEST-COMPILE-FAIL
//@ backends: gam
fn unused<T>() -> i32 { //~ ERROR unused generic parameter
    42
}

fn main() -> i32 {
    unused::<i32>() - 42
}
