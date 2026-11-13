//! M2.4 consumer walkthrough: the scaling analyzer — the same pipeline
//! `glyim-bench scale [--sizes ...] -- CMD...` runs, ending in the
//! `glyim.bench-scaling/1` JSON contract for the same series.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-bench --example scaling_fit
//! ```
//!
//! The example builds a synthetic 5-point series two ways: an exact linear
//! power law stored as `glyim.bench/1` reports in the M2.5
//! [`ReportStore`](glyim_bench::diff::ReportStore) (the offline wiring a
//! committed CI report history takes), and a live `run_scaling` pass over
//! a real subprocess (`echo` with a `{size}` placeholder — the same argv
//! substitution the `scale` subcommand performs). Both fits render their
//! text and JSON faces. An optional first argument overrides the size
//! list CSV (mirroring `--sizes`).

use std::path::PathBuf;

use glyim_bench::diff::{InMemoryReportStore, ReportStore};
use glyim_bench::report::{BenchReport, BenchSample, GateOutcome};
use glyim_bench::scaling::{
    ScalingPlan, ScalingThresholds, execute_scaling, fit_scaling, run_scaling,
};
use glyim_bench::source::MeasurementSource;
use glyim_bench::stats::SampleStats;

/// A wall-only `glyim.bench/1` report with a constant mean (the fixtures
/// the example stores).
fn fixture_report(program: &str, mean_ns: u64) -> BenchReport {
    let samples: Vec<BenchSample> = (0..10)
        .map(|_| BenchSample {
            wall_ns: mean_ns,
            instructions: None,
            cycles: None,
            rss_kb: None,
            alloc_count: None,
            alloc_bytes: None,
        })
        .collect();
    BenchReport::new(
        program,
        Vec::new(),
        MeasurementSource::WallOnly,
        10,
        samples,
        None,
        SampleStats {
            min: mean_ns,
            max: mean_ns,
            mean: mean_ns as f64,
            median: mean_ns as f64,
            cov_pct: 0.0,
        },
        GateOutcome {
            metric: "wall_ns".to_string(),
            cov_pct: 0.0,
            max_cov_pct: 0.1,
            passed: true,
        },
    )
}

fn main() {
    let sizes_csv = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10,100,1000,10000,100000".to_string());
    let sizes: Vec<u64> = sizes_csv
        .split(',')
        .map(|element| {
            element
                .trim()
                .parse()
                .expect("--sizes CSV of positive integers")
        })
        .collect();

    // 1. Offline face: an exact linear power law (2.0 ns per input item)
    //    stored through the M2.5 ReportStore seam, then fitted.
    let store = InMemoryReportStore::new();
    let mut paths = Vec::new();
    for size in &sizes {
        let path = PathBuf::from(format!("synthetic-{size}.json"));
        let mean = 2 * size;
        store
            .save(&path, &fixture_report("synthetic", mean))
            .expect("stores");
        paths.push((*size, path));
    }
    let outcome = execute_scaling(&store, &paths, &ScalingThresholds::new(1.0)).expect("fits");
    print!("{}", outcome.render_text());
    let json = outcome.to_json_pretty().expect("serializes");
    println!("{json}");

    // 2. Live face: a real subprocess measured at every size via the M2.2
    //    runner (tiny iterations; the fit is over echo's ~constant wall
    //    time, so the budget is generous).
    let plan = ScalingPlan::new(2, sizes, vec!["echo".to_string(), "{size}".to_string()])
        .with_max_exponent(1.0e6);
    let live = run_scaling(&plan).expect("runs");
    println!("live fit over `echo {{size}}`:");
    print!("{}", live.render_text());
    let fit = fit_scaling(&live.samples).expect("refits identically");
    assert_eq!(fit, live.fit, "plan C6: the fit is a pure function");
}
