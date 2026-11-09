// FnDef -> FnPtr coercion in value position: `let f: fn(..) = add`.
//@ mode: run-pass
//@ expect-exit: 37
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, LANG-TY-FN-PTR, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 {
    let f: fn(i32, i32) -> i32 = add;
    f(11, 26)
}
