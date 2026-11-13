//! Consumer walkthrough: the history trend analyzer (plan M2.5 complement —
//! total drift across the committed window).
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bench --example history_trend
//! ```
//!
//! The example reproduces the regression class the latest-baseline diff can
//! never see: every recorded run lands within the per-run budget while the
//! program still ratchets +1% per run. Ten runs are appended to a JSONL
//! history through [`JsonlHistoryStore`], the trend engine measures the
//! window's first-to-last drift through [`execute_trend_from_history`], and
//! a stable program is trended alongside to show the pass side of the same
//! gate.

use glyim_bench::error::BenchError;
use glyim_bench::history::{HistoryRecord, HistoryStore, JsonlHistoryStore};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};
use glyim_bench::trend::{TrendThresholds, execute_trend_from_history};

/// A report with constant samples so the means are exact.
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
        10
    ];
    BenchReport::new(
        program,
        Vec::new(),
        MeasurementSource::PerfStat,
        10,
        samples,
        Some(summarize(&[mean_ns * 2; 10]).expect("non-empty")),
        summarize(&[mean_ns; 10]).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() -> Result<(), BenchError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let history_path = dir.path().join("perf/history.jsonl");
    std::fs::create_dir_all(history_path.parent().expect("parent")).expect("mkdir");

    // Ten recorded runs on `main`: each is a perfectly stable, gate-passing
    // measurement, and each is exactly +1% slower than the last — the
    // ratchet shape. A second program stays flat.
    let history = JsonlHistoryStore;
    for i in 0..10 {
        history.append(
            &history_path,
            &HistoryRecord::new(report("ratcheting", 1_000_000 + i * 10_000)),
        )?;
        history.append(
            &history_path,
            &HistoryRecord::new(report("stable", 2_000_000)),
        )?;
    }

    // The windowed drift for both programs (last 8 runs each).
    let thresholds = TrendThresholds::new(8, 5.0);
    for program in ["ratcheting", "stable"] {
        let trend = execute_trend_from_history(&history, &history_path, program, &thresholds)?;
        println!("{}", trend.render_text());
        println!("---");
        if trend.is_ok() {
            println!("gate OK: `{program}` drifts within the +5% window budget");
        } else {
            println!(
                "gate FAILED: `{program}` accumulated {:+.1}% drift across the window \
                 while every individual run passed its own budget — this is the \
                 accumulated-ratchet class the latest-baseline diff cannot see \
                 (CI would exit non-zero)",
                trend.wall_drift_pct
            );
        }
        println!("======");
    }
    Ok(())
}
