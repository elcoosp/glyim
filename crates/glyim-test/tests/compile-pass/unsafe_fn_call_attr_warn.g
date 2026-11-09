// §3.5.30 attribute resolution: explicit `#[warn(..)]` still warns.
//@ mode: compile-pass
//@ expect-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SCOPE-ATTR-ALLOW, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

#[warn(unsafe_fn_call)]
fn main() -> i32 {
    get_value()
}
