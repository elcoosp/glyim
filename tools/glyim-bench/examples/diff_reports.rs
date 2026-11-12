//! Consumer walkthrough: persist two `glyim.bench/1` reports through the
//! [`ReportStore`] seam and diff them (plan M2.5, the perf-history gate).
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bench --example diff_reports
//! ```
//!
//! The example is the library counterpart of the CLI's `diff BASE CURRENT`
//! subcommand: a baseline report (stable) and a current report that
//! regressed beyond the default 5 % budget, loaded back through
//! [`FileReportStore`] and compared by [`execute_diff`]. The regression
//! verdict is what a CI perf job maps to a non-zero exit.

use glyim_bench::diff::{DiffThresholds, FileReportStore, ReportStore, execute_diff};
use glyim_bench::error::BenchError;
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::summarize;

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
        MeasurementSource::WallOnly,
        10,
        samples,
        Some(summarize(&[instructions; 10]).expect("non-empty")),
        summarize(&[mean_ns; 10]).expect("non-empty"),
        GateOutcome {
            metric: "instructions".to_string(),
            cov_pct: 0.0,
            max_cov_pct: glyim_bench::stats::DEFAULT_MAX_COV_PCT,
            passed: true,
        },
    )
}

fn main() -> Result<(), BenchError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let baseline_path = dir.path().join("baseline.json");
    let current_path = dir.path().join("current.json");

    let store = FileReportStore;
    store.save(
        &baseline_path,
        &report("glyim --emit=run corpus/large.g", 1_000_000, 2_000_000),
    )?;
    store.save(
        &current_path,
        &report("glyim --emit=run corpus/large.g", 1_120_000, 2_010_000),
    )?;

    let outcome = execute_diff(
        &store,
        &baseline_path,
        &current_path,
        &DiffThresholds::new(5.0),
    )?;

    print!("{}", outcome.render_text());
    println!("---");
    println!("{}", outcome.to_json_pretty()?);
    println!("---");
    if outcome.is_ok() {
        println!("gate OK: no metric regressed beyond +5%");
        Ok(())
    } else {
        println!("gate FAILED: wall time regressed beyond the budget (CI would exit non-zero)");
        Ok(())
    }
}
