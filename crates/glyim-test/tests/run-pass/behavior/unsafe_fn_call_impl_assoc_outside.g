// §3.5.30 item 2: calling an inherent associated `unsafe fn`
// (`S::get_value()`) outside any `unsafe { }` block must compile
// (warn) and run.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-IMPL, LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
struct S;

impl S {
    unsafe fn get_value() -> i32 {
        42
    }
}

fn main() -> i32 {
    S::get_value()
}
