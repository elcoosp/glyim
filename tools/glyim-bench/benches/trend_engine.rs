//! Trend engine bench (plan M2.5 complement).
//!
//! Dependency-free (`harness = false`), mirroring the `history_store` and
//! `matrix_engine` benches: measures the trend engine over a 256-record
//! committed history with a 32-record window — the program filter, the
//! window truncation, both drift figures, and the gate — so CI logs can
//! track the drift-ratchet's cost as the committed history grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench trend_engine
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::history::{HistoryRecord, HistoryStore, InMemoryHistoryStore};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};
use glyim_bench::trend::{TrendThresholds, trend_over_history};

const RECORDS: usize = 256;
const WINDOW: usize = 32;

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
    // The committed history: 256 runs with a +10 ns mean drift per record
    // (the accumulated-ratchet shape the trend gate exists to catch), plus
    // interleaved records of a second program the filter must skip.
    let records: Vec<HistoryRecord> = (0..RECORDS)
        .flat_map(|i| {
            [
                HistoryRecord::new(report("drifting", 1_000_000 + i as u64 * 10)),
                HistoryRecord::new(report("stable", 2_000_000)),
            ]
        })
        .collect();

    // The pure engine over the full record slice.
    let started = Instant::now();
    let trend = black_box(
        trend_over_history(
            black_box(&records),
            black_box("drifting"),
            &TrendThresholds::new(WINDOW, 5.0),
        )
        .expect("trends"),
    );
    let engine_elapsed = started.elapsed();

    // The store round trip (load_all) that feeds the engine in production.
    let store = InMemoryHistoryStore::new();
    let path = std::path::Path::new("bench/trend-history.jsonl");
    for record in black_box(&records) {
        store.append(path, record).expect("appends");
    }
    let started = Instant::now();
    let loaded = black_box(store.load_all(black_box(path)).expect("loads"));
    let load_elapsed = started.elapsed();

    // Contract asserts: the window is the last 32 records, the drift is
    // the +0.31% the 10 ns-per-record ratchet produced over the window,
    // and the interleaved program never entered the window.
    assert_eq!(loaded.len(), RECORDS * 2);
    assert_eq!(trend.window, WINDOW);
    assert_eq!(trend.records_total, RECORDS);
    let expected_first = 1_000_000.0 + (RECORDS - WINDOW) as f64 * 10.0;
    assert_eq!(trend.first_wall_mean, expected_first);
    let expected_drift = (WINDOW - 1) as f64 * 10.0 / expected_first * 100.0;
    assert!(
        (trend.wall_drift_pct - expected_drift).abs() < 1e-9,
        "drift {} vs {expected_drift}",
        trend.wall_drift_pct
    );
    assert!(
        trend.passed,
        "a +0.31% window drift is within the +5% budget"
    );

    println!("trend bench: {RECORDS} records x2 programs (load → filter → window → drift)");
    println!("  engine: {engine_elapsed:?}");
    println!("  load:   {load_elapsed:?}");
}
