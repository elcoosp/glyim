// P1-C2 batch 1.5: full 8-method x 4-type coverage of the integer bit
// family. Before this: `leading_ones` / `trailing_ones` did not exist
// (typeck: "no method"); `u8` had only `is_power_of_two`; `u64` had
// 2 of 6; and a duplicate `prim_bit_methods` block re-registered 14
// entries into a second id range (dead code, now removed).
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;

    // leading_ones
    if (0 as u32).leading_ones() != 0 { fails = fails + 1; }
    if (0xFFFFFFFF as u32).leading_ones() != 32 { fails = fails + 1; }
    if (0x80000000 as u32).leading_ones() != 1 { fails = fails + 1; }
    if (0xF0000000 as u32).leading_ones() != 4 { fails = fails + 1; }
    if (0xFF as u8).leading_ones() != 8 { fails = fails + 1; }
    if (0xF0 as u8).leading_ones() != 4 { fails = fails + 1; }
    if (0x0F as u8).leading_ones() != 0 { fails = fails + 1; }
    if (0 as u64).leading_ones() != 0 { fails = fails + 1; }
    if (1 as u64).leading_ones() != 0 { fails = fails + 1; }

    // trailing_ones
    if (0 as u32).trailing_ones() != 0 { fails = fails + 1; }
    if (0xFFFFFFFF as u32).trailing_ones() != 32 { fails = fails + 1; }
    if (1 as u32).trailing_ones() != 1 { fails = fails + 1; }
    if (0x0F as u32).trailing_ones() != 4 { fails = fails + 1; }
    if (0xFF as u8).trailing_ones() != 8 { fails = fails + 1; }
    if (0x01 as u8).trailing_ones() != 1 { fails = fails + 1; }
    if (0xF0 as u8).trailing_ones() != 0 { fails = fails + 1; }
    if (1 as u64).trailing_ones() != 1 { fails = fails + 1; }

    // u8 fills the existing 6
    if (4 as u8).count_ones() != 1 { fails = fails + 1; }
    if (0 as u8).count_zeros() != 8 { fails = fails + 1; }
    if (0 as u8).leading_zeros() != 8 { fails = fails + 1; }
    if (0 as u8).trailing_zeros() != 8 { fails = fails + 1; }
    if (100 as u8).next_power_of_two() != 128 { fails = fails + 1; }

    // u64 fills the existing 6
    if (5 as u64).count_ones() != 2 { fails = fails + 1; }
    if (0 as u64).count_zeros() != 64 { fails = fails + 1; }
    if (1 as u64).leading_zeros() != 63 { fails = fails + 1; }
    if (0x100 as u64).trailing_zeros() != 8 { fails = fails + 1; }
    if (1000 as u64).next_power_of_two() != 1024 { fails = fails + 1; }

    fails
}
