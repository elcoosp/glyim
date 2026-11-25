//! Generation-throughput bench — the plan M4.3 `DoD` line
//! "≥ 10k programs/hour/core".
//!
//! Dependency-free (`harness = false`), mirroring the repo's bench
//! convention (`std::time::Instant` + `std::hint::black_box`). Each
//! profile generates a mixed workload sized to complete in a few
//! milliseconds, then the harness prints programs/second/core; the `DoD`
//! bar of 10k programs/hour/core is ~2.8 programs/second/core, so the
//! printed number must clear that bar by orders of magnitude.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p glyim-smith --bench generation
//! ```

use glyim_smith::{GeneratorProfile, MAX_SIZE, generate};
use std::hint::black_box;
use std::time::Instant;

/// How many programs one measured pass generates per profile.
const PASS_PROGRAMS: u32 = 512;

fn main() {
    let mut worst_per_second = f64::INFINITY;
    for profile in GeneratorProfile::ALL {
        // Mixed sizes: the throughput claim must hold across the budget
        // range, not just for the cheapest program.
        let sizes = [1usize, 16, MAX_SIZE];
        let started = Instant::now();
        let mut generated = 0u32;
        for program_index in 0..PASS_PROGRAMS {
            let size = sizes[(program_index % sizes.len() as u32) as usize];
            let seed = u64::from(program_index);
            let program =
                generate(black_box(*profile), black_box(seed), black_box(size)).expect("in-bounds");
            generated += 1;
            // Keep the source alive so release builds cannot elide the
            // generation work.
            black_box(program.source.len());
            black_box(program.expected_exit);
        }
        let per_second = f64::from(generated) / started.elapsed().as_secs_f64();
        worst_per_second = worst_per_second.min(per_second);
        println!(
            "{:>20}: {per_second:>10.0} programs/second/core",
            profile.as_str()
        );
    }
    println!(
        "{:>20}: {worst_per_second:>10.0} programs/second/core",
        "worst profile"
    );
    println!(
        "{:>20}: {worst_per_second:>10.0} programs/hour/core",
        "worst profile (hour)"
    );
    println!(
        "plan M4.3 `DoD` bar: 10_000 programs/hour/core — {}",
        if worst_per_second * 3600.0 >= 10_000.0 {
            "CLEARED"
        } else {
            "MISSED"
        }
    );
}
