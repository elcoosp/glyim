// §3.5.36-adjacent: bare `Ok(x)` / `Err(x)` must resolve at the crate root.
//@ mode: run-pass
//@ compile-flags: --with-stdlib
//@ expect-exit: 42
//@ feature: STD-CORE-RESULT, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let r: Result<i32, i32> = Ok(42);
    match r { Ok(x) => x, Err(_) => 0 }
}
