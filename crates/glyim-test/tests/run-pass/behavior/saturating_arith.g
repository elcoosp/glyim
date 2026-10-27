// P1-C2 batch 2b: `saturating_add` / `saturating_sub` / `saturating_mul`
// on unsigned integers. Rust semantics: overflow clamps at the type's
// MAX (add/mul) or MIN = 0 (sub) rather than wrapping.
// Previously every call hit the P1-C2 gate for add/sub; mul was not
// registered at all.
//
// NOTE: every literal in an overflow test must carry an explicit suffix.
// An unsuffixed `4294967296 as u64` parses as `i32` first (the default
// integer type), overflows that, wraps to 0, *then* casts to u64 — a
// separate compiler gap tracked in KNOWN_GAPS §3.5.17 (out-of-range
// unsuffixed literals are silently wrapped, where Rust errors). Suffixing
// sidesteps the gap entirely so this fixture tests saturating arithmetic,
// not literal typing.
// test-mode: run-pass
// exit-code: 0
// compile-flags: --with-stdlib
fn main() -> i32 {
    let mut fails = 0;

    // u32 saturation at MAX
    let umax: u32 = 4294967295u32;
    if umax.saturating_add(1u32) != umax { fails = fails + 1; }
    if (0u32).saturating_sub(1u32) != 0u32 { fails = fails + 1; }
    if (65536u32).saturating_mul(65536u32) != umax { fails = fails + 1; }

    // u32 in-range
    if (5u32).saturating_add(3u32) != 8u32 { fails = fails + 1; }
    if (10u32).saturating_sub(4u32) != 6u32 { fails = fails + 1; }
    if (6u32).saturating_mul(7u32) != 42u32 { fails = fails + 1; }

    // u64 saturation at MAX
    let lmax: u64 = 18446744073709551615u64;
    if lmax.saturating_add(1u64) != lmax { fails = fails + 1; }
    if (0u64).saturating_sub(1u64) != 0u64 { fails = fails + 1; }
    if (4294967296u64).saturating_mul(4294967296u64) != lmax { fails = fails + 1; }

    // u64 in-range
    if (100u64).saturating_add(23u64) != 123u64 { fails = fails + 1; }
    if (100u64).saturating_sub(23u64) != 77u64 { fails = fails + 1; }
    if (11u64).saturating_mul(11u64) != 121u64 { fails = fails + 1; }

    // usize saturation + in-range
    if (0usize).saturating_sub(1usize) != 0usize { fails = fails + 1; }
    if (5usize).saturating_add(3usize) != 8usize { fails = fails + 1; }
    if (6usize).saturating_mul(7usize) != 42usize { fails = fails + 1; }

    fails
}
