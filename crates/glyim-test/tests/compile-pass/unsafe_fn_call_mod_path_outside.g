// §3.5.30 item 4: `mod::f` unsafe call outside `unsafe { }` must warn.
//@ mode: compile-pass
//@ expect-warning: call to unsafe function
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SCOPE-MOD-DECL, LANG-SYN-FN-DEF, TOOL-TEST-COMPILE-PASS
//@ backends: gam
mod m {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    m::get_value()
}
