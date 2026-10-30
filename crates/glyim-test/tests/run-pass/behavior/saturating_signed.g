// §3.5.19 follow-up: signed (i32/i64) saturating arithmetic, and
// regression coverage for the unsigned path.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    // Signed i32
    let a: i32 = 2000000000;
    if a.saturating_mul(2) != 2147483647 { return 1; }
    let b: i32 = -2000000000;
    if b.saturating_mul(2) != -2147483648 { return 2; }
    let d: i32 = 2147483647;
    if d.saturating_add(1) != 2147483647 { return 3; }
    let e: i32 = -2147483648;
    if e.saturating_sub(1) != -2147483648 { return 4; }
    let f: i32 = 100;
    if f.saturating_mul(2) != 200 { return 5; }
    // Signed i64
    let g: i64 = 9223372036854775807;
    if g.saturating_mul(2) != 9223372036854775807 { return 6; }
    let h: i64 = -9223372036854775808;
    if h.saturating_mul(2) != -9223372036854775808 { return 7; }
    // Unsigned regression — must still clamp at UINT_MAX
    let ua: u32 = 4294967295;
    if ua.saturating_add(1) != 4294967295 { return 8; }
    let ub: u32 = 65536;
    if ub.saturating_mul(65536) != 4294967295 { return 9; }
    let uc: u32 = 0;
    if uc.saturating_sub(1) != 0 { return 10; }
    0
}
