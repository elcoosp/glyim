//! Consumer walkthrough for the M2.2 measurement harness: measure a library
//! workload in-process with the counting allocator and the wall clock, gate
//! it, and print both renderings of the report.
//!
//! This is the "adopt the harness as a library" path that `glyim-test`'s
//! evidence emitter (M2.3) will use to fill its reserved `instructions` /
//! `rss_kb` columns: install [`CountingAllocator`], reset the counters around
//! the measured section, snapshot, and let the gate decide whether the
//! numbers are trustworthy at all. Runs anywhere — no `perf`/`valgrind`
//! needed.
//!
//! ```text
//! cargo run -p glyim-bench --example measure_alloc
//! ```

use std::time::Instant;

use glyim_bench::alloc::{alloc_snapshot, reset_alloc_stats};
use glyim_bench::counting_allocator;
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, coefficient_of_variation, gate_passes, summarize};

// In-process measurement requires the counting allocator; install it for
// this example binary.
counting_allocator!();

/// The measured workload: build and drop a small vector-of-vectors shape
/// (deterministic allocation volume).
fn workload() -> usize {
    let mut total = 0usize;
    let mut outer: Vec<Vec<u64>> = Vec::with_capacity(64);
    for i in 0..64u64 {
        let inner: Vec<u64> = (0..(i % 8) + 1).collect();
        total += inner.len();
        outer.push(inner);
    }
    drop(outer);
    total
}

fn main() {
    // Warm-up (also sanity: the workload is deterministic).
    assert_eq!(workload(), 288, "workload allocation shape is pinned");

    const RUNS: usize = 10;
    let mut samples: Vec<BenchSample> = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        reset_alloc_stats();
        let start = Instant::now();
        let total = std::hint::black_box(workload());
        let wall_ns = start.elapsed().as_nanos() as u64;
        let snapshot = alloc_snapshot();
        assert_eq!(total, 288);
        samples.push(BenchSample {
            wall_ns,
            instructions: None, // in-process runs have no counter wrapper
            cycles: None,
            rss_kb: None,
            alloc_count: Some(snapshot.count),
            alloc_bytes: Some(snapshot.bytes),
        });
    }

    let walls: Vec<u64> = samples.iter().map(|s| s.wall_ns).collect();
    let allocs: Vec<u64> = samples.iter().filter_map(|s| s.alloc_count).collect();
    let last_run_bytes = samples
        .last()
        .and_then(|s| s.alloc_bytes)
        .unwrap_or_default();
    let wall_stats = summarize(&walls).expect("runs > 0");
    let alloc_stats = summarize(&allocs).expect("runs > 0");
    let cov_pct = coefficient_of_variation(&allocs).unwrap_or(f64::NAN);
    // The primary counter for an in-process run is the allocation count:
    // deterministic for a deterministic workload, so the gate must pass.
    let gate = GateOutcome {
        metric: "alloc_count".into(),
        cov_pct,
        max_cov_pct: DEFAULT_MAX_COV_PCT,
        passed: gate_passes(cov_pct, DEFAULT_MAX_COV_PCT),
    };
    let report = BenchReport::new(
        "measure_alloc::workload",
        vec![],
        MeasurementSource::WallOnly,
        RUNS,
        samples,
        None,
        wall_stats,
        gate,
    );

    println!("{}", report.render_text());
    println!(
        "  alloc events: min {}, mean {:.1}, last-run gross bytes {}",
        alloc_stats.min, alloc_stats.mean, last_run_bytes
    );
    println!("\n-- glyim.bench/1 JSON --");
    match report.to_json_pretty() {
        Ok(json) => print!("{json}"),
        Err(err) => eprint!("{:?}", miette::Report::new(err)),
    }
    if !report.gate.passed {
        eprint!(
            "{:?}",
            miette::Report::new(glyim_bench::error::BenchError::GateFailed {
                metric: report.gate.metric.clone(),
                cov_pct: report.gate.cov_pct,
                max_cov_pct: report.gate.max_cov_pct,
            })
        );
        std::process::exit(1);
    }
}
