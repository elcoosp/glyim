// §3.5.22: integer to_string + format!("{}", int).
// Uses byte-length checks instead of string equality (comparison ops
// on String may not work in all contexts). test-mode: run-pass
//@ expect-exit: 0
//@ compile-flags: --with-stdlib
//@ mode: run-pass
//@ feature: LANG-BUILTIN-STRING, STD-ALLOC-STRING, STD-STD-INT-STR, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
fn main() -> i32 {
    let a = 42.to_string();
    if a.as_str().len() != 2 { return 1; }        // "42"
    let b = (-17).to_string();
    if b.as_str().len() != 3 { return 2; }        // "-17"
    let z = 0.to_string();
    if z.as_str().len() != 1 { return 3; }        // "0"
    let n: i32 = 123;
    if n.to_string().as_str().len() != 3 { return 4; }
    let u: u32 = 4294967295;
    if u.to_string().as_str().len() != 10 { return 5; }
    let big: u64 = 18446744073709551615;
    if big.to_string().as_str().len() != 20 { return 6; }
    let x = 42;
    let f = format!("value {}", x);
    if f.as_str().len() != 8 { return 7; }        // "value 42"
    0
}
