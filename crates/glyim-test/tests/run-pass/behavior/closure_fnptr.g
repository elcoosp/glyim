// A closure passed into a `fn(..)`-typed formal: the callee calls through
// the code pointer. Exercises the closure-value layout (`{ fn_ptr, captures }`)
// and the fn-pointer call path.
// test-mode: run-pass
// exit-code: 6
fn apply(f: fn(i32) -> i32, x: i32) -> i32 { f(x) }
fn main() -> i32 {
    apply(|x| x + 1, 5)
}
