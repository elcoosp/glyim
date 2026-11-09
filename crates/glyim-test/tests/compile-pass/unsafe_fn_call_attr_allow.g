// §3.5.30 attribute resolution: `#[allow(unsafe_fn_call)]` on the enclosing
// fn suppresses the warning.
//@ mode: compile-pass
//@ forbid-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SCOPE-ATTR-ALLOW, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

#[allow(unsafe_fn_call)]
fn main() -> i32 {
    get_value()
}
