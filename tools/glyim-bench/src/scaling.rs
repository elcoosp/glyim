//! Input-size scaling analysis (plan M2.4, the scaling-analyzer seam).
//!
//! The M2.2 harness measures one command at one input size; the M2.5 diff
//! compares two runs of the *same* size. Neither answers "does this program
//! stay linear as the input grows?" — that needs a *series* of sizes and a
//! fit. This module is that seam, split the way `diff.rs` separates input
//! sources from pure computation:
//!
//! - [`fit_scaling`] is the pure function `samples → (exponent, coefficient,
//!   r²)` — an ordinary least-squares fit over log(size) → log(mean wall
//!   time). No clock, no filesystem, deterministic for a given input
//!   (plan C6).
//! - [`substitute_size`] is the `{size}` argv-substitution contract that
//!   parameterizes a measured command per input size.
//! - [`run_scaling`] wires the M2.2 runner per size: substitute, run the
//!   iterations, take the wall mean, fit, and wrap the budget verdict.
//! - [`execute_scaling`] is the offline wiring: load per-size reports
//!   through the M2.5 [`ReportStore`](crate::diff::ReportStore) seam (the
//!   same store `diff` uses, so a committed report history doubles as a
//!   scaling-history input) and fit.
//!
//! Budget semantics mirror the M2.5 gate: an exponent **strictly above**
//! `max_exponent` is [`ScalingVerdict::Superlinear`] and fails the gate;
//! linear (`exponent == 1.0`), sublinear, and constant series pass.

use crate::error::BenchError;
use crate::runner::{RunPlan, detect_source, run_iterated};
use crate::source::MeasurementSource;

/// Schema tag pinned for the scaling outcome shape.
pub const BENCH_SCALING_SCHEMA: &str = "glyim.bench-scaling/1";

/// Default scaling budget: linear (`exponent <= 1.0`) passes. A CI perf
/// job passes an explicit `--max-exponent` when a sublinear-to-quadratic
/// envelope is the documented contract.
pub const DEFAULT_MAX_SCALING_EXPONENT: f64 = 1.0;

/// The argv placeholder `run_scaling` replaces with the input size.
pub const SIZE_PLACEHOLDER: &str = "{size}";

/// One point of the measured series: an input size and the mean wall time
/// observed at it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalingSample {
    /// The input size (must be positive; the fit works in log space).
    pub size: u64,
    /// Mean wall-clock time over the iterations at this size, in
    /// nanoseconds.
    pub mean_ns: f64,
}

/// The fitted power law `mean_ns ≈ coefficient · size^exponent`, plus its
/// goodness of fit.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalingFit {
    /// The exponent of the fitted power law (1.0 = linear, 2.0 =
    /// quadratic, 0.0 = constant).
    pub exponent: f64,
    /// The multiplicative coefficient (`mean_ns / size^exponent`).
    pub coefficient: f64,
    /// Coefficient of determination over the log-log regression; a flat
    /// series (zero variance) is reported as `1.0` by convention — the
    /// constant model explains it exactly.
    pub r_squared: f64,
    /// Number of samples the fit consumed.
    pub n_points: usize,
}

/// The scaling budget: a fitted exponent above `max_exponent` fails the
/// gate (strictly-above comparison, matching [`crate::diff::DiffThresholds`]
/// semantics).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalingThresholds {
    /// Allowed exponent (e.g. `1.0` allows linear and below).
    pub max_exponent: f64,
}

impl ScalingThresholds {
    /// A budget of `max_exponent` (e.g. `1.0` fails a quadratic fit).
    #[must_use]
    pub const fn new(max_exponent: f64) -> Self {
        Self { max_exponent }
    }
}

/// Verdict for the fitted series, relative to the budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ScalingVerdict {
    /// The exponent is within the budget (linear, sublinear, or constant).
    WithinBudget,
    /// The exponent exceeds the budget: the program scales superlinearly.
    Superlinear,
}

/// The full scaling outcome: envelope, the measured series, the fit, and
/// the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalingOutcome {
    /// Schema tag, always [`BENCH_SCALING_SCHEMA`].
    pub schema: String,
    /// The measured program (the first element of the live command, or the
    /// reports' shared program for the offline wiring).
    pub program: String,
    /// The measured series, sorted by size ascending.
    pub samples: Vec<ScalingSample>,
    /// The log-log fit over [`ScalingOutcome::samples`].
    pub fit: ScalingFit,
    /// The applied exponent budget.
    pub max_exponent: f64,
    /// The budget verdict for the fit.
    pub verdict: ScalingVerdict,
    /// Whether the fit stayed within the budget.
    pub passed: bool,
}

impl ScalingOutcome {
    /// Assembles the outcome around a finished fit: sorts the samples by
    /// size, applies the budget verdict, and pins the schema tag.
    #[must_use]
    pub fn new(
        program: impl Into<String>,
        mut samples: Vec<ScalingSample>,
        fit: ScalingFit,
        thresholds: ScalingThresholds,
    ) -> Self {
        samples.sort_by_key(|sample| sample.size);
        let verdict = if fit.exponent > thresholds.max_exponent {
            ScalingVerdict::Superlinear
        } else {
            ScalingVerdict::WithinBudget
        };
        Self {
            schema: BENCH_SCALING_SCHEMA.to_string(),
            program: program.into(),
            samples,
            fit,
            max_exponent: thresholds.max_exponent,
            verdict,
            passed: verdict == ScalingVerdict::WithinBudget,
        }
    }

    /// Ledger-convention alias for the gate: `true` when the fit is within
    /// the budget.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.passed
    }

    /// Pretty-printed JSON (deterministic field order via the struct shape).
    ///
    /// # Errors
    ///
    /// [`BenchError::ReportParse`] if serialisation fails (cannot happen
    /// for in-memory values, but kept for the `Result`-shaped seam).
    pub fn to_json_pretty(&self) -> Result<String, BenchError> {
        serde_json::to_string_pretty(self)
            .map(|mut text| {
                text.push('\n');
                text
            })
            .map_err(|err| BenchError::ReportParse {
                detail: err.to_string(),
            })
    }

    /// Fixed-width human-readable table (deterministic for a given
    /// outcome).
    #[must_use]
    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "glyim-bench scaling ({})", self.schema);
        let _ = writeln!(out, "  program : {}", self.program);
        let _ = writeln!(
            out,
            "  budget  : scaling exponent <= {:.3}",
            self.max_exponent
        );
        for sample in &self.samples {
            let _ = writeln!(out, "  size {}: {:.1} ns mean", sample.size, sample.mean_ns);
        }
        let _ = writeln!(
            out,
            "  fit: scaling exponent {:.3} coefficient {:.3} r2 {:.4} ({} points)",
            self.fit.exponent, self.fit.coefficient, self.fit.r_squared, self.fit.n_points,
        );
        let _ = writeln!(
            out,
            "  verdict: {}",
            match self.verdict {
                ScalingVerdict::WithinBudget => "within budget",
                ScalingVerdict::Superlinear => "SUPERLINEAR",
            }
        );
        let _ = writeln!(
            out,
            "  gate: {}",
            if self.passed { "PASSED" } else { "FAILED" }
        );
        out
    }
}

/// Ordinary least squares over `(log size, log mean wall time)`.
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] when fewer than two samples are given, a
/// size or mean is non-positive (the fit works in log space), or sizes are
/// not distinct.
pub fn fit_scaling(samples: &[ScalingSample]) -> Result<ScalingFit, BenchError> {
    if samples.len() < 2 {
        return Err(BenchError::InvalidPlan {
            detail: format!(
                "a scaling fit needs at least 2 sizes, got {}",
                samples.len()
            ),
        });
    }
    let points: Vec<(f64, f64)> = samples
        .iter()
        .map(|sample| {
            if sample.size == 0 {
                return Err(BenchError::InvalidPlan {
                    detail: "scaling sizes must be positive (the fit works in log space)"
                        .to_string(),
                });
            }
            if sample.mean_ns <= 0.0 {
                return Err(BenchError::InvalidPlan {
                    detail: format!(
                        "scaling means must be positive, got {} ns at size {}",
                        sample.mean_ns, sample.size
                    ),
                });
            }
            Ok((f64::ln(sample.size as f64), f64::ln(sample.mean_ns)))
        })
        .collect::<Result<_, _>>()?;
    let n = points.len();
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n as f64;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n as f64;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    let mut sxy = 0.0;
    for (x, y) in &points {
        let dx = x - mean_x;
        let dy = y - mean_y;
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxx == 0.0 {
        return Err(BenchError::InvalidPlan {
            detail: "scaling sizes must be distinct (log(size) has zero variance)".to_string(),
        });
    }
    // A flat series (every mean identical) has zero variance in log(mean):
    // the constant model explains it exactly, so report exponent 0.0 and
    // r² 1.0 by convention instead of a floating-point roundoff residue.
    // The y values are compared exactly: they are the same `ln` result for
    // identical inputs, so equality is a value identity, not an epsilon.
    let flat = points.windows(2).all(|pair| pair[0].1 == pair[1].1);
    let (exponent, r_squared) = if flat {
        (0.0, 1.0)
    } else {
        (sxy / sxx, (sxy * sxy) / (sxx * syy))
    };
    let coefficient = f64::exp(mean_y - exponent * mean_x);
    Ok(ScalingFit {
        exponent,
        coefficient,
        r_squared,
        n_points: n,
    })
}

/// Replaces [`SIZE_PLACEHOLDER`] in every argv element with `size`.
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] when no argument carries the placeholder
/// (running the identical command at every size would fit noise, not
/// scaling).
pub fn substitute_size(command: &[String], size: u64) -> Result<Vec<String>, BenchError> {
    let substituted: Vec<String> = command
        .iter()
        .map(|arg| arg.replace(SIZE_PLACEHOLDER, &size.to_string()))
        .collect();
    let parameterized = command.iter().any(|arg| arg.contains(SIZE_PLACEHOLDER));
    if !parameterized {
        return Err(BenchError::InvalidPlan {
            detail: format!(
                "the measured command carries no {SIZE_PLACEHOLDER} placeholder; \
                 every size would measure the identical command"
            ),
        });
    }
    Ok(substituted)
}

/// A scaling measurement plan: the M2.2 runner settings plus the size
/// series and the parameterized command.
#[derive(Clone, Debug)]
pub struct ScalingPlan {
    /// Iterations per size (the M2.2 gate expects 10).
    pub iterations: usize,
    /// Counter source; `None` auto-detects `perf`, falling back to wall.
    pub source: Option<MeasurementSource>,
    /// The input sizes to measure (any order; duplicates rejected).
    pub sizes: Vec<u64>,
    /// The exponent budget for the final verdict.
    pub max_exponent: f64,
    /// The measured command; one argument must carry
    /// [`SIZE_PLACEHOLDER`].
    pub command: Vec<String>,
}

impl ScalingPlan {
    /// Plans a scaling run at the given sizes.
    #[must_use]
    pub fn new(iterations: usize, sizes: Vec<u64>, command: Vec<String>) -> Self {
        Self {
            iterations,
            source: None,
            sizes,
            max_exponent: DEFAULT_MAX_SCALING_EXPONENT,
            command,
        }
    }

    /// Pins the counter source instead of auto-detecting.
    #[must_use]
    pub const fn with_source(mut self, source: MeasurementSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Overrides the exponent budget.
    #[must_use]
    pub const fn with_max_exponent(mut self, max_exponent: f64) -> Self {
        self.max_exponent = max_exponent;
        self
    }

    /// Sorted sizes with duplicates rejected (one measurement per size).
    fn sorted_sizes(&self) -> Result<Vec<u64>, BenchError> {
        if self.sizes.is_empty() {
            return Err(BenchError::InvalidPlan {
                detail: "at least one size is required".to_string(),
            });
        }
        let mut sizes = self.sizes.clone();
        sizes.sort_unstable();
        if sizes.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(BenchError::InvalidPlan {
                detail: "scaling sizes must be distinct".to_string(),
            });
        }
        Ok(sizes)
    }
}

/// Measures the command at every size through the M2.2 runner and fits the
/// series (the live wiring).
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] for an empty command or duplicate/empty
/// size list, [`BenchError::InvalidPlan`] from
/// [`substitute_size`] when no argument carries
/// [`SIZE_PLACEHOLDER`], and any runner error from the iterations.
pub fn run_scaling(plan: &ScalingPlan) -> Result<ScalingOutcome, BenchError> {
    let _run_span = tracing::info_span!(
        "bench_scaling",
        sizes = plan.sizes.len(),
        iterations = plan.iterations,
    )
    .entered();
    if plan.command.is_empty() {
        return Err(BenchError::InvalidPlan {
            detail: "no command given; usage: glyim-bench scale -- <program> [{size} args..]"
                .to_string(),
        });
    }
    let sizes = plan.sorted_sizes()?;
    let source = plan.source.unwrap_or_else(detect_source);
    let mut samples = Vec::with_capacity(sizes.len());
    for size in sizes {
        let substituted = substitute_size(&plan.command, size)?;
        let (program, args) = substituted
            .split_first()
            .ok_or_else(|| BenchError::InvalidPlan {
                detail: "empty substituted command".to_string(),
            })?;
        let mut runner_plan = RunPlan::new(program)
            .with_iterations(plan.iterations)
            .with_source(source);
        for arg in args {
            runner_plan = runner_plan.push_arg(arg);
        }
        let report = run_iterated(&runner_plan)?;
        tracing::debug!(size, mean_ns = report.wall.mean, "scaling sample collected");
        samples.push(ScalingSample {
            size,
            mean_ns: report.wall.mean,
        });
    }
    let fit = fit_scaling(&samples)?;
    Ok(ScalingOutcome::new(
        plan.command[0].clone(),
        samples,
        fit,
        ScalingThresholds::new(plan.max_exponent),
    ))
}

/// Fits a stored report history through the M2.5
/// [`ReportStore`](crate::diff::ReportStore) seam (the offline wiring: the
/// reports were measured earlier — e.g. committed by CI — and only the fit
/// runs now).
///
/// # Errors
///
/// Store load failures, and [`BenchError::InvalidPlan`] when the reports
/// measure different programs (mirroring the diff engine's program check)
/// or the series is degenerate for [`fit_scaling`].
pub fn execute_scaling(
    store: &dyn crate::diff::ReportStore,
    samples: &[(u64, std::path::PathBuf)],
    thresholds: &ScalingThresholds,
) -> Result<ScalingOutcome, BenchError> {
    let mut measured: Vec<ScalingSample> = Vec::with_capacity(samples.len());
    let mut program: Option<String> = None;
    for (size, path) in samples {
        let report = store.load(path)?;
        let report_program = report.program.clone();
        match &program {
            Some(expected) if *expected != report_program => {
                return Err(BenchError::InvalidPlan {
                    detail: format!(
                        "report {} measures `{}` but earlier reports measure `{}`; \
                         a scaling series must measure one program",
                        path.display(),
                        report_program,
                        expected
                    ),
                });
            }
            _ => program = Some(report_program),
        }
        measured.push(ScalingSample {
            size: *size,
            mean_ns: report.wall.mean,
        });
    }
    let fit = fit_scaling(&measured)?;
    Ok(ScalingOutcome::new(
        program.unwrap_or_default(),
        measured,
        fit,
        *thresholds,
    ))
}
