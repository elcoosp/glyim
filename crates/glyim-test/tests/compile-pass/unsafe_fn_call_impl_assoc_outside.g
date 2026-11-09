// §3.5.30 item 4: `Type::assoc` unsafe call outside `unsafe { }` must warn.
//@ mode: compile-pass
//@ expect-warning: call to unsafe function
//@ feature: LANG-SYN-IMPL, LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    S::get_value()
}
