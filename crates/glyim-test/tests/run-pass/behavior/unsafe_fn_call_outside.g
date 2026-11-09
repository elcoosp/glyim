// §3.5.30 MVP: calling a declared-unsafe free function outside any
// `unsafe { }` block must compile (warning-only in the MVP) and run.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-UNSAFE-BLOCK, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
unsafe fn get_value() -> i32 {
    42
}

fn main() -> i32 {
    get_value()
}
