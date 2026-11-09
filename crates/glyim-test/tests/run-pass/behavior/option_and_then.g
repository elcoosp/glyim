// and_then: closure returning Option<U> — nested ADT position.
//@ mode: run-pass
//@ expect-exit: 9
//@ compile-flags: --with-stdlib
//@ feature: LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let o = Option::Some(3);
    o.and_then(|x| Option::Some(x + 6)).unwrap()
}
