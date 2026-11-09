// `String::from_utf8` result matched, with `.len()` on the `Ok` payload.
// Regression: this shape used to fail typeck with
// "ambiguous method `len` found in multiple impls", and the `from_utf8`
// lowering once read the length word through a mis-scaled GEP.
//@ mode: run-pass
//@ expect-exit: 1
//@ compile-flags: --with-stdlib
//@ feature: LANG-MATCH-GUARDS, LANG-BUILTIN-STRING, STD-ALLOC-STRING, RUNTIME-FFI-VECSTR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let mut buf = Vec::new();
    buf.push(65u8);
    let s = String::from_utf8(buf);
    match s {
        Result::Ok(v) => v.len() as i32,
        Result::Err(_) => 0,
    }
}
