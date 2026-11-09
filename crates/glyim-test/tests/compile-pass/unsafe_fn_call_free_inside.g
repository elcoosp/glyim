// §3.5.30 item 4: free-fn unsafe call inside `unsafe { }` must NOT warn.
//@ mode: compile-pass
//@ forbid-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    unsafe { get_value() }
}
