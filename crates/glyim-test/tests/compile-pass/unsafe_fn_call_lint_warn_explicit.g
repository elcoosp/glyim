// §3.5.30 framework: `-W unsafe` is a no-op (default already warns).
//@ mode: compile-pass
//@ compile-flags: -W unsafe
//@ expect-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
