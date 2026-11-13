//! Matrix engine bench (plan §6 M2.5 remainder).
//!
//! Dependency-free (`harness = false`), mirroring the `history_store` and
//! `budget_engine` benches: measures the matrix gate end-to-end over an
//! 8-program benchmark matrix (the `budget_engine` matrix size) with a
//! 256-record committed history — baseline selection per program, the
//! per-row M2.5 diffs, and both geo-mean aggregates — so CI logs can track
//! the perf-compare gate's cost as the matrix and history grow. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench matrix_engine
//! ```

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use glyim_bench::diff::DiffVerdict;
use glyim_bench::history::{HistoryRecord, HistoryStore, InMemoryHistoryStore};
use glyim_bench::matrix::{
    BaselineSource, HistoryBaselineSource, MatrixThresholds, diff_matrix, geo_mean_delta_pct,
};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};

const PROGRAMS: usize = 8;
const RECORDS: usize = 256;

/// One `glyim.bench/1` report over a constant sample set.
fn report(program: &str, mean_ns: u64) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: mean_ns,
            instructions: Some(mean_ns * 2),
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        };
        16
    ];
    BenchReport::new(
        program,
        Vec::new(),
        MeasurementSource::PerfStat,
        16,
        samples,
        summarize(&[mean_ns * 2; 16]),
        summarize(&[mean_ns; 16]).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() {
    // The committed history: 256 runs per program with slowly drifting
    // means (file order is append order, plan C6). The latest record's
    // mean is 1_000_000 + (RECORDS - 1) * 100 = 1_025_500.
    let history = InMemoryHistoryStore::new();
    let history_path = Path::new("bench/matrix-history.jsonl");
    for i in 0..RECORDS {
        for p in 0..PROGRAMS {
            let program = format!("bench-{p}");
            let record = HistoryRecord::new(report(&program, 1_000_000 + i as u64 * 100));
            history.append(history_path, &record).expect("appends");
        }
    }
    let stored = history.load_all(history_path).expect("loads");
    assert_eq!(stored.len(), PROGRAMS * RECORDS);

    // The current matrix: one report per program, uniformly +1% against
    // every latest baseline (1_025_500 · 101 / 100 = 1_035_755, exact in
    // u64) — the plan's uniform-drift scenario: within every
    // per-benchmark budget, over the geo-mean budget.
    let current: Vec<(PathBuf, BenchReport)> = (0..PROGRAMS)
        .map(|p| {
            let program = format!("bench-{p}");
            (
                PathBuf::from(format!("{program}.json")),
                report(&program, 1_025_500 * 101 / 100),
            )
        })
        .collect();

    // Baseline selection per program through the seam (cached load).
    let started = Instant::now();
    let baselines = HistoryBaselineSource::new(&history, history_path);
    for (_, bench_report) in black_box(&current) {
        let baseline = baselines
            .baseline_for(&bench_report.program)
            .expect("baseline");
        black_box(baseline);
    }
    let select_elapsed = started.elapsed();

    // The full matrix gate: rows, verdicts, both geo-means.
    let started = Instant::now();
    let outcome = black_box(
        diff_matrix(
            black_box(&current),
            &baselines,
            &MatrixThresholds::new(2.0, 0.5),
        )
        .expect("matrix diffs"),
    );
    let matrix_elapsed = started.elapsed();

    // The aggregate math in isolation.
    let started = Instant::now();
    let deltas: Vec<f64> = outcome
        .rows
        .iter()
        .map(|row| row.diff.wall.delta_pct)
        .collect();
    let geo = black_box(geo_mean_delta_pct(black_box(&deltas)));
    let geo_elapsed = started.elapsed();

    // Contract asserts: 8 rows, the +1% drift is within every
    // per-benchmark budget and over the geo-mean budget — the exact class
    // the plan's aggregate clause exists to catch.
    assert_eq!(outcome.rows.len(), PROGRAMS);
    assert!(
        outcome
            .rows
            .iter()
            .all(|row| row.diff.wall.verdict == DiffVerdict::Unchanged),
        "uniform +1% stays within the +2% per-benchmark budget"
    );
    assert!(
        (geo - 1.0).abs() < 1e-6,
        "uniform +1% drift has a +1% geo-mean, got {geo}"
    );
    assert!(
        geo > 0.5,
        "the geo-mean budget flags what no per-benchmark budget catches"
    );

    println!("matrix bench: {PROGRAMS} programs x {RECORDS} history records (select → diff → geo)");
    println!("  baseline select: {select_elapsed:?}");
    println!("  matrix diff:     {matrix_elapsed:?}");
    println!("  geo-mean:        {geo_elapsed:?}");
}
