//! Consumer walkthrough: the append-only perf history (plan §6 M2.5,
//! `perf/history/*.jsonl`) and the latest-baseline diff.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bench --example history_baseline
//! ```
//!
//! The example is the library counterpart of the CLI's `history record` /
//! `history diff` pair: two `glyim.bench/1` reports are appended to a JSONL
//! history through [`JsonlHistoryStore`], the current report is diffed
//! against the **latest** recorded baseline through
//! [`execute_diff_from_history`] — file order is append order, so the
//! choice is deterministic without a clock — and the regression verdict is
//! what a CI perf job maps to a non-zero exit.

use glyim_bench::diff::{DiffThresholds, FileReportStore, ReportStore};
use glyim_bench::error::BenchError;
use glyim_bench::history::{
    HistoryStore, JsonlHistoryStore, execute_diff_from_history, latest_baseline, program_slug,
};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::{DEFAULT_MAX_COV_PCT, summarize};

/// A report with constant samples so the means are exact.
fn report(program: &str, mean_ns: u64, instructions: u64) -> BenchReport {
    let samples = vec![
        BenchSample {
            wall_ns: mean_ns,
            instructions: Some(instructions),
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
        Some(summarize(&[instructions; 10]).expect("non-empty")),
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
    let program = "glyim --emit=run corpus/large.g";
    let history_path = dir
        .path()
        .join(format!("perf/history/{}.jsonl", program_slug(program)));
    std::fs::create_dir_all(history_path.parent().expect("parent")).expect("mkdir");
    let current_path = dir.path().join("current.json");

    let reports = FileReportStore;
    let history = JsonlHistoryStore;

    // Commit two runs on `main`: an older one and the latest baseline.
    let older = report(program, 1_000_000, 2_000_000);
    let latest = report(program, 1_100_000, 2_100_000);
    history.append(
        &history_path,
        &glyim_bench::history::HistoryRecord::new(older),
    )?;
    history.append(
        &history_path,
        &glyim_bench::history::HistoryRecord::new(latest),
    )?;
    reports.save(&current_path, &report(program, 1_240_000, 2_050_000))?;

    // The pure selection: the latest record wins (file order = append order).
    let records = history.load_all(&history_path)?;
    let baseline = latest_baseline(&records, program)?;
    println!(
        "baseline: wall mean {:.0} ns (latest of {} records)",
        baseline.wall.mean,
        records.len()
    );

    // The wiring: diff the current report against that baseline.
    let outcome = execute_diff_from_history(
        &reports,
        &history,
        &history_path,
        &current_path,
        &DiffThresholds::new(5.0),
    )?;

    print!("{}", outcome.render_text());
    println!("---");
    if outcome.is_ok() {
        println!("gate OK: no metric regressed beyond +5% against the recorded history");
        Ok(())
    } else {
        println!(
            "gate FAILED: the current run regressed beyond the budget (CI would exit non-zero)"
        );
        Ok(())
    }
}
