// §3.5.37: multi-arg format! / println! substitution.
//
// Before: format!("a={} b={}", 42, 100) printed empty (the whole block
// fell through to the literal-concat fallback because try_interp only
// handled ONE {} then rejected multi-{} format strings). After: the
// expander loops over every {} and emits one push_str per substitution.
//@ mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ feature: STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    // 2 substitutions
    let s = format!("a={} b={}", 42, 100);
    if s.as_str().len() != 10 { return 1; } // "a=42 b=100"
    // 3 substitutions
    let t = format!("{} + {} = {}", 1, 2, 3);
    if t.as_str().len() != 9 { return 2; } // "1 + 2 = 3"
    // single-substitution regression
    let u = format!("x={}", 7);
    if u.as_str().len() != 3 { return 3; } // "x=7"
    // trailing text after last substitution
    let v = format!("{}!", 42);
    if v.as_str().len() != 3 { return 4; } // "42!"
    // println! multi-arg through the same expansion path
    println!("p={} q={}", 5, 7);
    0
}
