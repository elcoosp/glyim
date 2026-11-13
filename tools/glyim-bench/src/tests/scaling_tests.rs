//! Integration tests for the M2.4 scaling analyzer: the pure log-log fit,
//! the `{size}` argv substitution, the live runner wiring, and the offline
//! `ReportStore` wiring (the M2.5 seam reused for stored histories).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diff::ReportStore;
use crate::error::BenchError;
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::scaling::{
    DEFAULT_MAX_SCALING_EXPONENT, ScalingOutcome, ScalingSample, ScalingThresholds, ScalingVerdict,
    execute_scaling, fit_scaling, run_scaling, substitute_size,
};
use crate::source::MeasurementSource;
use crate::stats::SampleStats;

// -- helpers ------------------------------------------------------------

/// Builds a wall-only report with the given mean wall time, mirroring the
/// `diff_tests` fixture shape.
fn report(program: &str, mean_ns: u64) -> BenchReport {
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

/// An `InMemoryReportStore` stand-in built from `(size, report)` pairs; the
/// real store types are exercised in `diff_tests`, so this keeps the
/// scaling tests focused on the fit and the wiring.
struct FixtureStore {
    reports: RefCell<BTreeMap<PathBuf, BenchReport>>,
}

impl FixtureStore {
    fn new(pairs: &[(u64, BenchReport)]) -> Self {
        Self {
            reports: RefCell::new(
                pairs
                    .iter()
                    .map(|(size, report)| (PathBuf::from(format!("{size}.json")), report.clone()))
                    .collect(),
            ),
        }
    }

    fn paths(&self) -> Vec<(u64, PathBuf)> {
        self.reports
            .borrow()
            .keys()
            .map(|path| {
                let size = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.parse::<u64>().ok())
                    .expect("fixture paths are `<size>.json`");
                (size, path.clone())
            })
            .collect()
    }
}

impl ReportStore for FixtureStore {
    fn load(&self, path: &Path) -> Result<BenchReport, BenchError> {
        self.reports
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| BenchError::ReportRead {
                path: path.display().to_string(),
                detail: "not found in the fixture store".to_string(),
            })
    }

    fn save(&self, path: &Path, report: &BenchReport) -> Result<(), BenchError> {
        let _ = report.to_json_pretty()?;
        self.reports
            .borrow_mut()
            .insert(path.to_path_buf(), report.clone());
        Ok(())
    }
}

/// Mean wall times from the exact power law `c * n^p` (f64-exact for the
/// chosen sizes; `powf` keeps fractional exponents such as 0.5 exact on
/// perfect-square sizes).
fn power_law(size: u64, coefficient: f64, exponent: f64) -> f64 {
    coefficient * f64::powf(size as f64, exponent)
}

// -- engine: the log-log fit ----------------------------------------------

#[test]
fn fit_recovers_a_linear_power_law_exactly() {
    let sizes = [100_u64, 1_000, 10_000, 100_000];
    let samples: Vec<ScalingSample> = sizes
        .iter()
        .map(|size| ScalingSample {
            size: *size,
            mean_ns: power_law(*size, 2.5, 1.0),
        })
        .collect();
    let fit = fit_scaling(&samples).expect("fits");
    assert!(
        (fit.exponent - 1.0).abs() < 1e-9,
        "exponent {}",
        fit.exponent
    );
    assert!(
        (fit.coefficient - 2.5).abs() < 1e-9,
        "coefficient {}",
        fit.coefficient
    );
    assert!(fit.r_squared > 0.999_999, "r2 {}", fit.r_squared);
    assert_eq!(fit.n_points, 4);
}

#[test]
fn fit_recovers_superlinear_and_sublinear_laws() {
    for (exponent, size_list) in [
        (2.0_f64, [1_u64, 10, 100, 1_000]),
        (0.5_f64, [100_u64, 400, 900, 1_600]),
    ] {
        let samples: Vec<ScalingSample> = size_list
            .iter()
            .map(|size| ScalingSample {
                size: *size,
                mean_ns: power_law(*size, 3.0, exponent),
            })
            .collect();
        let fit = fit_scaling(&samples).expect("fits");
        assert!(
            (fit.exponent - exponent).abs() < 1e-9,
            "expected {exponent}, got {}",
            fit.exponent
        );
    }
}

#[test]
fn fit_reports_constant_time_as_exponent_zero() {
    let samples: Vec<ScalingSample> = [10_u64, 1_000, 100_000]
        .iter()
        .map(|size| ScalingSample {
            size: *size,
            mean_ns: 500.0,
        })
        .collect();
    let fit = fit_scaling(&samples).expect("fits");
    assert_eq!(fit.exponent, 0.0, "a flat series is constant time");
}

#[test]
fn fit_rejects_degenerate_inputs() {
    let one_point = || {
        vec![ScalingSample {
            size: 10,
            mean_ns: 5.0,
        }]
    };
    assert!(matches!(
        fit_scaling(&one_point()),
        Err(BenchError::InvalidPlan { .. })
    ));
    let duplicate_sizes = || {
        vec![
            ScalingSample {
                size: 10,
                mean_ns: 5.0,
            },
            ScalingSample {
                size: 10,
                mean_ns: 50.0,
            },
        ]
    };
    assert!(matches!(
        fit_scaling(&duplicate_sizes()),
        Err(BenchError::InvalidPlan { .. })
    ));
    let zero_size = || {
        vec![
            ScalingSample {
                size: 0,
                mean_ns: 5.0,
            },
            ScalingSample {
                size: 10,
                mean_ns: 50.0,
            },
        ]
    };
    assert!(matches!(
        fit_scaling(&zero_size()),
        Err(BenchError::InvalidPlan { .. })
    ));
    let zero_mean = || {
        vec![
            ScalingSample {
                size: 10,
                mean_ns: 0.0,
            },
            ScalingSample {
                size: 100,
                mean_ns: 50.0,
            },
        ]
    };
    assert!(matches!(
        fit_scaling(&zero_mean()),
        Err(BenchError::InvalidPlan { .. })
    ));
}

// -- engine: verdict + outcome ----------------------------------------------

#[test]
fn verdicts_follow_the_exponent_budget() {
    let outcome_for = |mean_ns: fn(u64) -> f64| {
        let samples: Vec<ScalingSample> = [100_u64, 10_000]
            .iter()
            .map(|size| ScalingSample {
                size: *size,
                mean_ns: mean_ns(*size),
            })
            .collect();
        let fit = fit_scaling(&samples).expect("fits");
        ScalingOutcome::new(
            "bench",
            samples,
            fit,
            ScalingThresholds::new(DEFAULT_MAX_SCALING_EXPONENT),
        )
    };
    let linear = outcome_for(|size| power_law(size, 2.0, 1.0));
    assert_eq!(linear.verdict, ScalingVerdict::WithinBudget);
    assert!(linear.is_ok());
    let quadratic = outcome_for(|size| power_law(size, 2.0, 2.0));
    assert_eq!(quadratic.verdict, ScalingVerdict::Superlinear);
    assert!(!quadratic.is_ok());
}

#[test]
fn outcome_json_round_trips_strictly_and_deterministically() {
    let samples: Vec<ScalingSample> = [1_u64, 10, 100]
        .iter()
        .map(|size| ScalingSample {
            size: *size,
            mean_ns: power_law(*size, 1.5, 2.0),
        })
        .collect();
    let fit = fit_scaling(&samples).expect("fits");
    let outcome = ScalingOutcome::new("bench", samples, fit, ScalingThresholds::new(1.0));
    let json = outcome.to_json_pretty().expect("serializes");
    assert!(
        json.contains("\"schema\": \"glyim.bench-scaling/1\""),
        "{json}"
    );
    let back: ScalingOutcome = serde_json::from_str(&json).expect("round trips");
    assert_eq!(back, outcome);
    // Determinism: re-render is byte-identical (plan C6).
    assert_eq!(outcome.to_json_pretty().unwrap(), json);
    assert_eq!(outcome.render_text(), outcome.render_text());
}

#[test]
fn text_render_names_the_fit_and_verdict() {
    let samples: Vec<ScalingSample> = [1_u64, 10, 100]
        .iter()
        .map(|size| ScalingSample {
            size: *size,
            mean_ns: power_law(*size, 1.5, 2.0),
        })
        .collect();
    let fit = fit_scaling(&samples).expect("fits");
    let outcome = ScalingOutcome::new("bench", samples, fit, ScalingThresholds::new(1.0));
    let text = outcome.render_text();
    assert!(text.contains("glyim.bench-scaling/1"), "{text}");
    assert!(text.contains("scaling exponent"), "{text}");
    assert!(text.contains("SUPERLINEAR"), "{text}");
    assert!(text.contains("gate: FAILED"), "{text}");
    // Samples are sorted by size in the table.
    let first = text.find("size 1:").expect("size 1 row");
    let second = text.find("size 10:").expect("size 10 row");
    let third = text.find("size 100:").expect("size 100 row");
    assert!(first < second && second < third);
}

// -- argv substitution --------------------------------------------------------

#[test]
fn substitution_replaces_the_placeholder_everywhere() {
    let substituted = substitute_size(
        &[
            "./bench".to_string(),
            "--n".to_string(),
            "{size}".to_string(),
        ],
        42,
    )
    .expect("substitutes");
    assert_eq!(
        substituted,
        vec!["./bench".to_string(), "--n".to_string(), "42".to_string()]
    );
    let repeated = substitute_size(&["{size}x{size}".to_string()], 7).expect("substitutes");
    assert_eq!(repeated, vec!["7x7".to_string()]);
}

#[test]
fn substitution_requires_a_placeholder() {
    let err = substitute_size(&["./bench".to_string(), "--fast".to_string()], 42)
        .expect_err("no placeholder");
    assert!(matches!(err, BenchError::InvalidPlan { .. }));
    assert!(
        err.to_string().contains("{size}"),
        "the error names the placeholder: {err}"
    );
}

// -- wiring: live runs ---------------------------------------------------------

#[test]
fn run_scaling_measures_every_size_in_order() {
    let plan = crate::scaling::ScalingPlan::new(
        2,
        vec![8, 4, 2],
        vec!["echo".to_string(), "{size}".to_string()],
    )
    .with_max_exponent(1.0e6);
    let outcome = run_scaling(&plan).expect("runs");
    let sizes: Vec<u64> = outcome.samples.iter().map(|s| s.size).collect();
    assert_eq!(
        sizes,
        [2, 4, 8],
        "sorted ascending regardless of plan order"
    );
    assert!(outcome.is_ok(), "generous budget: {:?}", outcome.fit);
    assert_eq!(outcome.fit.n_points, 3);
}

#[test]
fn run_scaling_rejects_duplicate_sizes_and_empty_commands() {
    let dup = crate::scaling::ScalingPlan::new(
        2,
        vec![8, 8],
        vec!["echo".to_string(), "{size}".to_string()],
    );
    assert!(matches!(
        run_scaling(&dup),
        Err(BenchError::InvalidPlan { .. })
    ));
    let empty = crate::scaling::ScalingPlan::new(2, vec![8], Vec::new());
    assert!(matches!(
        run_scaling(&empty),
        Err(BenchError::InvalidPlan { .. })
    ));
}

// -- wiring: the M2.5 ReportStore reuse ----------------------------------------

#[test]
fn execute_scaling_fits_a_stored_history() {
    let store = FixtureStore::new(&[(100, report("bench", 1_000)), (10, report("bench", 100))]);
    let outcome = execute_scaling(
        &store,
        &store.paths(),
        &ScalingThresholds::new(DEFAULT_MAX_SCALING_EXPONENT),
    )
    .expect("fits");
    assert!(
        (outcome.fit.exponent - 1.0).abs() < 1e-9,
        "{}",
        outcome.fit.exponent
    );
    assert_eq!(outcome.program, "bench");
    assert!(outcome.is_ok());
}

#[test]
fn execute_scaling_fails_on_missing_or_mismatched_reports() {
    let store = FixtureStore::new(&[(10, report("bench", 100)), (100, report("bench", 1_000))]);
    let mut paths = store.paths();
    paths.push((1_000, PathBuf::from("absent.json")));
    assert!(matches!(
        execute_scaling(&store, &paths, &ScalingThresholds::new(1.0)),
        Err(BenchError::ReportRead { .. })
    ));
    let mismatched =
        FixtureStore::new(&[(10, report("bench", 100)), (100, report("other", 1_000))]);
    assert!(matches!(
        execute_scaling(
            &mismatched,
            &mismatched.paths(),
            &ScalingThresholds::new(1.0)
        ),
        Err(BenchError::InvalidPlan { .. })
    ));
}
