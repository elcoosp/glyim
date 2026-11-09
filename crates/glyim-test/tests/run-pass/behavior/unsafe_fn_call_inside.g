// §3.5.30 MVP: same call inside an `unsafe { }` block must compile with
// no warning (unsafe_depth > 0 suppresses it) and run identically.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    unsafe { get_value() }
}
