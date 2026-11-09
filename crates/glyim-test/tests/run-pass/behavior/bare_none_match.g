// §3.5.36-adjacent: bare `None` in a match arm.
//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-exit: 0
//@ feature: LANG-MATCH-GUARDS, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o: Option<i32> = None;
    match o { Some(x) => x, None => 0 }
}
