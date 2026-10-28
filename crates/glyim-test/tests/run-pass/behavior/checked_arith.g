// P1-C2 batch 2c: `checked_add` / `checked_sub` / `checked_mul` on
// unsigned integers. Each returns `Option<T>` — `None` on overflow,
// `Some(result)` otherwise.
//
// Literals carry explicit suffixes for the same reason documented in
// `saturating_arith.g` (§3.5.18): an unsuffixed `4294967296` types as
// i32 first and is rejected by the range check.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;

    // ---- u32 ----
    // Overflow at MAX
    if (4294967295u32).checked_add(1u32).is_some() { fails = fails + 1; }
    // In range
    if (5u32).checked_add(3u32).is_none() { fails = fails + 1; }
    // Underflow at 0
    if (0u32).checked_sub(1u32).is_some() { fails = fails + 1; }
    // In range
    if (10u32).checked_sub(4u32).is_none() { fails = fails + 1; }
    // Mul overflow
    if (65536u32).checked_mul(65536u32).is_some() { fails = fails + 1; }
    // Mul in range
    if (6u32).checked_mul(7u32).is_none() { fails = fails + 1; }

    // ---- u64 ----
    if (18446744073709551615u64).checked_add(1u64).is_some() { fails = fails + 1; }
    if (100u64).checked_add(23u64).is_none() { fails = fails + 1; }
    if (0u64).checked_sub(1u64).is_some() { fails = fails + 1; }
    if (100u64).checked_sub(23u64).is_none() { fails = fails + 1; }
    if (4294967296u64).checked_mul(4294967296u64).is_some() { fails = fails + 1; }
    if (11u64).checked_mul(11u64).is_none() { fails = fails + 1; }

    // ---- usize ----
    if (0usize).checked_sub(1usize).is_some() { fails = fails + 1; }
    if (5usize).checked_add(3usize).is_none() { fails = fails + 1; }
    if (6usize).checked_mul(7usize).is_none() { fails = fails + 1; }

    fails
}
