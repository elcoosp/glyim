//! Perf-history store bench (plan §6 M2.5).
//!
//! Dependency-free (`harness = false`), mirroring the `diff_engine` and
//! `budget_engine` benches: measures the JSONL record round trip (serialise
//! and parse) plus the latest-baseline selection over a synthetic 256-record
//! history, so CI logs can track the store cost as the committed history
//! grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench history_store
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::history::{
    HistoryRecord, HistoryStore, InMemoryHistoryStore, latest_baseline, program_slug,
};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};

const RECORDS: usize = 256;

/// One `glyim.bench/1` report over a constant sample set.
fn report(wall_mean: u64) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: wall_mean,
            instructions: Some(wall_mean * 2),
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        };
        16
    ];
    BenchReport::new(
        "glyim --emit=obj fib.g",
        Vec::new(),
        MeasurementSource::PerfStat,
        16,
        samples,
        summarize(&[wall_mean * 2; 16]),
        summarize(&[wall_mean; 16]).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() {
    // The synthetic history: 256 committed runs with slowly drifting means.
    let records: Vec<HistoryRecord> = (0..RECORDS)
        .map(|i| HistoryRecord::new(report(1_000_000 + i as u64 * 100)))
        .collect();

    // Round trip through the in-memory store: append + load_all.
    let store = InMemoryHistoryStore::new();
    let path = std::path::Path::new("bench/history.jsonl");

    let started = Instant::now();
    for record in black_box(&records) {
        store.append(path, record).expect("appends");
    }
    let append_elapsed = started.elapsed();

    let started = Instant::now();
    let loaded = black_box(store.load_all(black_box(path)).expect("loads"));
    let load_elapsed = started.elapsed();

    // The JSONL line contract (what the file store writes per append).
    let started = Instant::now();
    let lines: Vec<String> = loaded
        .iter()
        .map(|record| record.to_jsonl_line().expect("serialises"))
        .collect();
    let line_elapsed = started.elapsed();
    let started = Instant::now();
    let parsed: Vec<HistoryRecord> = lines
        .iter()
        .map(|line| HistoryRecord::from_jsonl_line(line).expect("parses"))
        .collect();
    let parse_elapsed = started.elapsed();

    // The baseline selection over the full history.
    let started = Instant::now();
    let baseline = black_box(
        latest_baseline(black_box(&loaded), black_box("glyim --emit=obj fib.g"))
            .expect("history exists"),
    );
    let select_elapsed = started.elapsed();

    // Contract asserts: file order is append order (plan C6), the latest
    // record wins, and the slug is filesystem-safe.
    assert_eq!(loaded.len(), RECORDS);
    assert_eq!(
        loaded[RECORDS - 1].report.wall.mean,
        1_000_000.0 + (RECORDS as f64 - 1.0) * 100.0
    );
    assert_eq!(parsed, loaded);
    assert_eq!(
        baseline.wall.mean,
        1_000_000.0 + (RECORDS as f64 - 1.0) * 100.0
    );
    assert_eq!(
        program_slug("glyim --emit=obj fib.g"),
        "glyim-emit-obj-fib-g"
    );

    println!("history bench: {RECORDS} committed records (append → load → line → parse → select)");
    println!("  append: {append_elapsed:?}");
    println!("  load:   {load_elapsed:?}");
    println!("  line:   {line_elapsed:?}");
    println!("  parse:  {parse_elapsed:?}");
    println!("  select: {select_elapsed:?}");
}
