// FnDef -> FnPtr coercion as a call argument (value-position constant
// flowing into a `fn(i32)->i32` parameter, then called through the
// FnPtr path inside the callee).
//@ mode: run-pass
//@ expect-exit: 53
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-RAW-PTR, STD-CORE-PTR, LANG-TY-FN-PTR, LANG-SYN-FN-DEF, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn inc(x: i32) -> i32 { x + 1 }
fn apply(f: fn(i32) -> i32, v: i32) -> i32 { f(v) }
fn main() -> i32 {
    apply(inc, 52)
}
