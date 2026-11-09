// §3.5.30 item 4: `Type::assoc` unsafe call inside `unsafe { }` must NOT warn.
//@ mode: compile-pass
//@ forbid-warning: call to unsafe function
//@ feature: LANG-SYN-IMPL, LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    unsafe { S::get_value() }
}
