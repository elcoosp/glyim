// P1-C2 batch 2a: `wrapping_add` / `wrapping_sub` / `wrapping_mul` on
// unsigned integers. Rust semantics: perform at infinite precision,
// truncate to the receiver's width. Previously every one of these calls
// hit the P1-C2 gate ("resolved by the type checker but not implemented
// by the LLVM backend") for add/sub, and "no method" for mul (mul was
// not even registered).
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;

    // u32 wrap-around at MAX
    let umax: u32 = 4294967295;
    let uone: u32 = 1;
    if umax.wrapping_add(uone) != 0 { fails = fails + 1; }
    if (0 as u32).wrapping_sub(uone) != 4294967295 { fails = fails + 1; }
    if umax.wrapping_mul(2 as u32) != 4294967294 { fails = fails + 1; }

    // u32 in-range (no wrap needed)
    if (5 as u32).wrapping_add(3 as u32) != 8 { fails = fails + 1; }
    if (10 as u32).wrapping_sub(4 as u32) != 6 { fails = fails + 1; }
    if (6 as u32).wrapping_mul(7 as u32) != 42 { fails = fails + 1; }

    // u64 wrap-around
    let lmax: u64 = 18446744073709551615;
    let lone: u64 = 1;
    if lmax.wrapping_add(lone) != 0 { fails = fails + 1; }
    if (0 as u64).wrapping_sub(lone) != 18446744073709551615 { fails = fails + 1; }
    if lmax.wrapping_mul(2 as u64) != 18446744073709551614 { fails = fails + 1; }

    // usize wrap-around (64-bit target)
    let sz: usize = 0;
    let so: usize = 1;
    if sz.wrapping_sub(so) != 18446744073709551615 { fails = fails + 1; }
    if so.wrapping_add(so) != 2 { fails = fails + 1; }
    if (7 as usize).wrapping_mul(6 as usize) != 42 { fails = fails + 1; }

    fails
}
