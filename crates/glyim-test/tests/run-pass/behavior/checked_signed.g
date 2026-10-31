// §3.5.32: signed (i32/i64) checked arithmetic + unsigned regression.
//
// Uses is_some()/is_none()/unwrap() rather than match-on-Option: match
// arms whose body is a bare `return` currently mis-type (`mismatched
// types: () vs i32`), a pre-existing shape separate from this feature.
// checked_arith.g uses is_some/is_none too — same pattern.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    // i32 add: MAX + -1 = Some(MAX-1) — the sadd trap
    let a: i32 = 2147483647;
    let r1 = a.checked_add(-1);
    if r1.is_none() { return 1; }
    if r1.unwrap() != 2147483646 { return 2; }
    // i32 add overflow
    if a.checked_add(1).is_some() { return 3; }
    // i32 sub: MIN - (-1) = Some(MIN+1) — ssub trap
    let b: i32 = -2147483648;
    let r2 = b.checked_sub(-1);
    if r2.is_none() { return 4; }
    if r2.unwrap() != -2147483647 { return 5; }
    // i32 sub underflow
    if b.checked_sub(1).is_some() { return 6; }
    // i32 mul overflow
    if a.checked_mul(2).is_some() { return 7; }
    // i32 mul MIN * -1 overflows
    let c: i32 = -1;
    if c.checked_mul(b).is_some() { return 8; }
    // i32 in-range mul
    let d: i32 = 10;
    let r3 = d.checked_mul(3);
    if r3.is_none() { return 9; }
    if r3.unwrap() != 30 { return 10; }
    // i64 add overflow
    let e: i64 = 9223372036854775807;
    if e.checked_add(1).is_some() { return 11; }
    // i64 add -1
    let r4 = e.checked_add(-1);
    if r4.is_none() { return 12; }
    if r4.unwrap() != 9223372036854775806 { return 13; }
    // Literal receiver (from §3.5.31(a) fix)
    let r5 = 100.checked_mul(2);
    if r5.is_none() { return 14; }
    if r5.unwrap() != 200 { return 15; }
    // Unsigned regression
    let ua: u32 = 4294967295;
    if ua.checked_add(1).is_some() { return 16; }
    0
}
