//! Harness-overhead bench for `glyim-bench` (M2.2).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-test`
//! `annotations`/`evidence` and `glyim-pipeline` `phase_prof` benches: the
//! harness must not distort what it measures, so this bench pins the cost of
//! the three in-process harness components — the statistics summary, the
//! report renderers, and the counting allocator's per-event overhead. Run
//! with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench runner_overhead
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::alloc::{alloc_snapshot, reset_alloc_stats};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{coefficient_of_variation, summarize};

// The bench measures the counting allocator's real per-event cost, so the
// bench binary adopts it (the library deliberately never installs one).
glyim_bench::counting_allocator!();

/// Samples per measurement loop (large enough for µs-scale costs).
const ITERATIONS: usize = 10_000;

/// Sample count per statistics call, shaped like a 10-run report.
const SAMPLES: usize = 10;

fn synthetic_samples() -> Vec<u64> {
    // Deterministic "instruction counts" with sub-threshold jitter: the
    // shape a healthy gated run has.
    (0..SAMPLES).map(|i| 1_000_000 + (i % 3) as u64).collect()
}

fn main() {
    let samples = synthetic_samples();

    // Component 1: statistics (summarize + CoV) over a 10-sample report.
    let t = Instant::now();
    for _ in 0..ITERATIONS {
        let stats = black_box(summarize(black_box(&samples)));
        black_box(stats.map(|s| s.cov_pct));
    }
    let stats_us = t.elapsed().as_micros() as f64 / ITERATIONS as f64;

    // Component 2: CoV alone (the gate hot path).
    let t = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(coefficient_of_variation(black_box(&samples)));
    }
    let cov_us = t.elapsed().as_micros() as f64 / ITERATIONS as f64;

    // Component 3: counting-allocator overhead — alloc/dealloc of one 64-byte
    // box, counter reads included.
    reset_alloc_stats();
    let t = Instant::now();
    for _ in 0..ITERATIONS {
        let boxed = black_box(Box::new([0u8; 64]));
        black_box(boxed);
    }
    let alloc_us = t.elapsed().as_micros() as f64 / ITERATIONS as f64;
    let alloc_events = alloc_snapshot().count;

    // Component 4: report text rendering (the CI-log table path).
    let stats = summarize(&samples).expect("samples are non-empty");
    let report = BenchReport::new(
        "synthetic",
        vec![],
        MeasurementSource::PerfStat,
        SAMPLES,
        samples
            .iter()
            .map(|&ins| BenchSample {
                wall_ns: ins,
                instructions: Some(ins),
                cycles: Some(ins / 3),
                rss_kb: Some(4_096),
                alloc_count: None,
                alloc_bytes: None,
            })
            .collect(),
        Some(stats.clone()),
        stats,
        GateOutcome {
            metric: "instructions".into(),
            cov_pct: 0.0002,
            max_cov_pct: 0.1,
            passed: true,
        },
    );
    let t = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(black_box(report.clone()).render_text());
    }
    let render_us = t.elapsed().as_micros() as f64 / ITERATIONS as f64;

    println!("runner_overhead bench ({SAMPLES}-sample report shape, {ITERATIONS} iterations)");
    println!("  summarize+stats     {stats_us:8.3} µs/call");
    println!("  cov (gate path)     {cov_us:8.3} µs/call");
    println!("  counted alloc+drop  {alloc_us:8.3} µs/call  ({alloc_events} events counted)");
    println!("  render_text         {render_us:8.3} µs/call");
}
