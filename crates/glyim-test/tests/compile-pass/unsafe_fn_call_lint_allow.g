// §3.5.30 framework: `-A unsafe` suppresses the warning.
//@ mode: compile-pass
//@ compile-flags: -A unsafe
//@ forbid-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SCOPE-ATTR-ALLOW, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
