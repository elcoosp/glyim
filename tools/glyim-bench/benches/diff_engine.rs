//! Diff-engine bench (M2.5 perf-history seam).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-ledger`
//! benches: measures the full diff path — store load, engine, both
//! renderers — over two synthetic 4096-sample `glyim.bench/1` reports
//! (the shape a committed perf baseline has), so CI logs can track the
//! gate cost as reports grow. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench diff_engine
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::diff::{DiffThresholds, DiffVerdict, FileReportStore, ReportStore, execute_diff};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::summarize;

/// Two reports over the same program: the current one ~20 % slower on
/// instructions (a regression the gate must catch) and ~8 % slower on wall
/// (within the 10 % wall budget? no — it regresses too; both rows assert).
fn synthetic_reports() -> (BenchReport, BenchReport) {
    let samples = |wall_ns: u64, instructions: u64| -> Vec<BenchSample> {
        (0..4096)
            .map(|i| BenchSample {
                wall_ns: wall_ns + i % 7,
                instructions: Some(instructions + i % 3),
                cycles: None,
                rss_kb: None,
                alloc_count: None,
                alloc_bytes: None,
            })
            .collect()
    };
    let wall_stats = |wall_ns: u64| {
        let set: Vec<u64> = (0..4096).map(|i| wall_ns + i % 7).collect();
        summarize(&set).expect("non-empty")
    };
    let instr_stats = |instructions: u64| {
        let set: Vec<u64> = (0..4096).map(|i| instructions + i % 3).collect();
        summarize(&set).expect("non-empty")
    };
    let baseline = BenchReport::new(
        "glyim --emit=run corpus/large.g",
        Vec::new(),
        MeasurementSource::PerfStat,
        4096,
        samples(1_000_000, 2_000_000),
        Some(instr_stats(2_000_000)),
        wall_stats(1_000_000),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.001,
            max_cov_pct: 0.1,
            passed: true,
        },
    );
    let current = BenchReport::new(
        "glyim --emit=run corpus/large.g",
        Vec::new(),
        MeasurementSource::PerfStat,
        4096,
        samples(1_080_000, 2_400_000),
        Some(instr_stats(2_400_000)),
        wall_stats(1_080_000),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.001,
            max_cov_pct: 0.1,
            passed: true,
        },
    );
    (baseline, current)
}

fn main() {
    let (baseline, current) = synthetic_reports();
    let dir = tempfile::tempdir().expect("tempdir");
    let baseline_path = dir.path().join("baseline.json");
    let current_path = dir.path().join("current.json");
    let store = FileReportStore;
    store
        .save(&baseline_path, &baseline)
        .expect("save baseline");
    store.save(&current_path, &current).expect("save current");

    // Gate correctness pins: +20 % instructions and +8 % wall both exceed
    // the 5 % budget, so the outcome must fail.
    let outcome = execute_diff(
        &store,
        &baseline_path,
        &current_path,
        &DiffThresholds::new(5.0),
    )
    .expect("both present");
    assert!(!outcome.is_ok());
    assert_eq!(
        outcome.diffs[0]
            .instructions
            .as_ref()
            .expect("both")
            .verdict,
        DiffVerdict::Regressed
    );

    let started = Instant::now();
    for _ in 0..200 {
        black_box(execute_diff(
            black_box(&store),
            black_box(&baseline_path),
            black_box(&current_path),
            &DiffThresholds::new(5.0),
        ))
        .expect("both present");
    }
    let load_and_diff_elapsed = started.elapsed();

    let started = Instant::now();
    for _ in 0..200 {
        black_box(black_box(&outcome).render_text());
    }
    let text_elapsed = started.elapsed();

    // Determinism: two renders are byte-identical (plan C6).
    assert_eq!(outcome.render_text(), outcome.render_text());

    println!(
        "diff engine bench: 2 x 4096-sample reports x 200 iters\n  load+diff : {load_and_diff_elapsed:?}\n  text render: {text_elapsed:?}"
    );
}
