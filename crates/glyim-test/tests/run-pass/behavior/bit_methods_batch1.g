// P1-C2 batch 1: `count_ones` / `count_zeros` / `leading_zeros` /
// `trailing_zeros` / `next_power_of_two` on unsigned integers.
// Every case below exercises a code path that previously hit the
// P1-C2 gate (`builtin method ... not implemented by the LLVM backend`).
// The overflow case of `next_power_of_two` (x > 2^(width-1)) is
// intentionally not exercised here; see the arm comment in
// codegen-llvm/src/lower.rs for the current wrap-to-0 behavior.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;

    // count_ones
    if (0 as u32).count_ones() != 0 { fails = fails + 1; }
    if (1 as u32).count_ones() != 1 { fails = fails + 1; }
    if (5 as u32).count_ones() != 2 { fails = fails + 1; }
    if (0xFFFFFFFF as u32).count_ones() != 32 { fails = fails + 1; }

    // leading_zeros
    if (0 as u32).leading_zeros() != 32 { fails = fails + 1; }
    if (1 as u32).leading_zeros() != 31 { fails = fails + 1; }
    if (0xFF as u32).leading_zeros() != 24 { fails = fails + 1; }
    if (0x80000000 as u32).leading_zeros() != 0 { fails = fails + 1; }

    // trailing_zeros
    if (0 as u32).trailing_zeros() != 32 { fails = fails + 1; }
    if (1 as u32).trailing_zeros() != 0 { fails = fails + 1; }
    if (0x100 as u32).trailing_zeros() != 8 { fails = fails + 1; }
    if (0x80000000 as u32).trailing_zeros() != 31 { fails = fails + 1; }

    // next_power_of_two
    if (0 as u32).next_power_of_two() != 1 { fails = fails + 1; }
    if (1 as u32).next_power_of_two() != 1 { fails = fails + 1; }
    if (2 as u32).next_power_of_two() != 2 { fails = fails + 1; }
    if (3 as u32).next_power_of_two() != 4 { fails = fails + 1; }
    if (4 as u32).next_power_of_two() != 4 { fails = fails + 1; }
    if (5 as u32).next_power_of_two() != 8 { fails = fails + 1; }
    if (1000 as u32).next_power_of_two() != 1024 { fails = fails + 1; }

    // count_zeros on usize (only type with this method registered).
    // 5 = 0b101 has 2 ones, so a 64-bit `usize` has 64 - 2 = 62 zeros.
    if (0 as usize).count_zeros() != 64 { fails = fails + 1; }
    if (5 as usize).count_zeros() != 62 { fails = fails + 1; }

    fails
}
