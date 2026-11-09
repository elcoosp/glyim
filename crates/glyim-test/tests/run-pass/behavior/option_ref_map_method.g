// Closure param must be seeded with the *resolved* formal (concrete
// `&str`), not a raw Infer var — else `p.len()` reports no-method.
//@ mode: run-pass
//@ expect-exit: 5
//@ compile-flags: --with-stdlib
//@ feature: LANG-TY-REF, LANG-BUILTIN-OPTION, STD-CORE-OPTION, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let s = "hello";
    let o: Option<&str> = Option::Some(s);
    o.map(|p| p.len() as i32).unwrap()
}
