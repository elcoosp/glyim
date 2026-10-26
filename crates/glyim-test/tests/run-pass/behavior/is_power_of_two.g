// Regression for P1-C2: `u32::is_power_of_two()` — Rust semantics
// `x != 0 && (x & (x - 1)) == 0`. Required by stdlib
// `Layout::from_size_align`, which every allocation path calls.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;
    if !(1 as u32).is_power_of_two() { fails = fails + 1; }
    if !(2 as u32).is_power_of_two() { fails = fails + 1; }
    if !(4 as u32).is_power_of_two() { fails = fails + 1; }
    if !(1024 as u32).is_power_of_two() { fails = fails + 1; }
    if (0 as u32).is_power_of_two() { fails = fails + 1; }
    if (3 as u32).is_power_of_two() { fails = fails + 1; }
    if (5 as u32).is_power_of_two() { fails = fails + 1; }
    if (1023 as u32).is_power_of_two() { fails = fails + 1; }
    fails
}
