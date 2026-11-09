// A closure passed into a `fn(..)`-typed formal: the callee calls through
// the code pointer. Exercises the closure-value layout (`{ fn_ptr, captures }`)
// and the fn-pointer call path.
//@ mode: run-pass
//@ expect-exit: 6
//@ feature: LANG-CLOSURE-EXPR, LANG-TY-FN-PTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn apply(f: fn(i32) -> i32, x: i32) -> i32 { f(x) }
fn main() -> i32 {
    apply(|x| x + 1, 5)
}
