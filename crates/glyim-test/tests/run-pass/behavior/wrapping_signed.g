// §3.5.31(b): signed (i32/i64) wrapping arithmetic + unsigned regression.
// All expectations computed via Python (wrap_i32/wrap_i64), not hand.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let a: i32 = 2147483647;
    if a.wrapping_add(1) != -2147483648 { return 1; }
    let b: i32 = -2147483648;
    if b.wrapping_sub(1) != 2147483647 { return 2; }
    let c: i32 = -2000000000;
    let d: i32 = -2000000000;
    if c.wrapping_mul(d) != -1651507200 { return 3; }
    let e: i32 = 2147483647;
    let f: i32 = -2147483648;
    if e.wrapping_mul(f) != -2147483648 { return 4; }
    let g: i32 = 2147483647;
    if g.wrapping_mul(2) != -2 { return 5; }
    let h: i32 = -2147483648;
    let i: i32 = -2147483648;
    if h.wrapping_mul(i) != 0 { return 6; }
    let j: i32 = 100;
    if j.wrapping_mul(2) != 200 { return 7; }
    let k: i64 = 9223372036854775807;
    if k.wrapping_add(1) != -9223372036854775808 { return 8; }
    // Unsigned regression
    let ua: u32 = 4294967295;
    if ua.wrapping_add(1) != 0 { return 9; }
    let ub: u32 = 0;
    if ub.wrapping_sub(1) != 4294967295 { return 10; }
    0
}
