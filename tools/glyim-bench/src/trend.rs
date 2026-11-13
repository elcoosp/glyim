//! The history trend analyzer (plan M2.5 complement: drift across the
//! committed window, §325 "History & gating").
//!
//! The M2.5 diff gate compares a current report against the **latest**
//! recorded baseline, and the matrix gate (see [`crate::matrix`]) aggregates
//! one PR's worth of comparisons. Neither can see the slowest regression
//! class there is: **accumulated drift**. When every PR lands within its own
//! budget and `main` re-records the baseline, each individual diff is green
//! while the program is still measurably slower six months later — a ratchet
//! that only turns in one direction. This module is the complementary view:
//! it reads the *whole committed window* for a program (the plan's
//! `perf/history/*.jsonl` records) and measures how far the window's first
//! record has drifted by its last.
//!
//! Split exactly like [`crate::diff`] and [`crate::matrix`]:
//!
//! - [`trend_over_history`] is the pure function `(records, program,
//!   budget) → drift verdicts` — no clock, no IO, records in append order
//!   (plan C6). The window is the **last** [`TrendThresholds::window`]
//!   records of the program; the drift is `(last − first) / first` over the
//!   window's wall means, and over its instruction means only when *every*
//!   point in the window measured them (the [`crate::matrix`] honesty rule:
//!   a metric computed over a subset is not coverage).
//! - [`execute_trend_from_history`] wires the stores: load the history
//!   through the [`crate::history::HistoryStore`] seam and wrap the verdict
//!   in a [`TrendReport`] gate.
//!
//! Gate semantics keep the house conventions: the inclusive `<=` boundary
//! (drift exactly at the budget passes), improvements are negative drift
//! and never fail, and the degenerate zero-first-mean case is an honest
//! infinite drift (the [`crate::diff`] engine's rule) — a gate failure, not
//! a silent pass.

use std::fmt::Write as _;
use std::path::Path;

use crate::error::BenchError;
use crate::history::{HistoryRecord, HistoryStore};
use crate::report::BenchReport;

/// Schema tag pinned for the trend report shape.
pub const BENCH_TREND_SCHEMA: &str = "glyim.bench-trend/1";

/// Default trend window, in records: the last 8 recorded runs of a program
/// (the matrix bench scale keeps the evidence table readable in CI logs).
pub const DEFAULT_TREND_WINDOW: usize = 8;

/// Default total-drift budget over the window, in percent. Deliberately
/// tighter than the per-benchmark diff default: the window spans many
/// recorded runs, so its *total* drift accumulates what the per-run budgets
/// each allowed.
pub const DEFAULT_MAX_TREND_DRIFT_PCT: f64 = 5.0;

/// The trend budgets: how many records the window spans and how much total
/// drift it may accumulate before the gate fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrendThresholds {
    /// Number of records the trend spans (the **last** N of the program's
    /// history; must be at least 2 — a trend needs two points).
    pub window: usize,
    /// Allowed total upward drift over the window, in percent.
    pub max_drift_pct: f64,
}

impl TrendThresholds {
    /// A window of `window` records with a total drift budget of
    /// `max_drift_pct` percent (the house defaults are
    /// [`DEFAULT_TREND_WINDOW`] / [`DEFAULT_MAX_TREND_DRIFT_PCT`]).
    #[must_use]
    pub const fn new(window: usize, max_drift_pct: f64) -> Self {
        Self {
            window,
            max_drift_pct,
        }
    }
}

/// One point of the trend: the means of one recorded run (the evidence
/// trail behind the drift numbers).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrendPoint {
    /// The recorded run's wall-clock mean, in nanoseconds.
    pub wall_mean: f64,
    /// The recorded run's instruction-count mean, when that run measured
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_mean: Option<f64>,
}

/// The drift report for one program's committed window: first/last means,
/// both drift figures, and the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrendReport {
    /// Schema tag, always [`BENCH_TREND_SCHEMA`].
    pub schema: String,
    /// The trended program.
    pub program: String,
    /// Records of this program in the history file (before windowing).
    pub records_total: usize,
    /// Records the trend actually spans (the window, truncated to the
    /// available history).
    pub window: usize,
    /// The window's first point's wall mean, in nanoseconds.
    pub first_wall_mean: f64,
    /// The window's last point's wall mean, in nanoseconds.
    pub last_wall_mean: f64,
    /// Total wall drift over the window, in percent.
    pub wall_drift_pct: f64,
    /// Total instruction drift over the window — `Some` only when *every*
    /// point in the window measured instructions (the honesty rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_drift_pct: Option<f64>,
    /// The applied total-drift budget, in percent.
    pub max_drift_pct: f64,
    /// The window's points, in append order (the evidence trail behind the
    /// drift figures).
    pub points: Vec<TrendPoint>,
    /// Whether both measured drifts are within the budget.
    pub passed: bool,
}

impl TrendReport {
    /// Ledger-convention alias for the gate: `true` when the drift is
    /// within budget.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.passed
    }

    /// Pretty-printed JSON (deterministic field order via the struct shape).
    ///
    /// # Errors
    ///
    /// [`BenchError::ReportParse`] if serialisation fails (cannot happen for
    /// in-memory values, but kept for the `Result`-shaped seam).
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

    /// Fixed-width human-readable table (deterministic for a given report).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "glyim-bench trend ({})", self.schema);
        let _ = writeln!(
            out,
            "  program: {} ({} of {} recorded run(s) in window)",
            self.program, self.window, self.records_total
        );
        for (index, point) in self.points_enumerated() {
            let instructions = point
                .instructions_mean
                .map_or_else(|| "-".to_string(), |mean| format!("{mean:.0}"));
            let _ = writeln!(
                out,
                "    {:<4} wall mean {:>12.0} ns  instructions {:>14}",
                index, point.wall_mean, instructions
            );
        }
        let _ = writeln!(
            out,
            "  wall drift: {:.2} -> {:.2} ({:+.3}%) over the window",
            self.first_wall_mean, self.last_wall_mean, self.wall_drift_pct
        );
        if let Some(drift) = self.instructions_drift_pct {
            let _ = writeln!(out, "  instructions drift: {drift:+.3}% over the window");
        }
        let _ = writeln!(
            out,
            "  budget: {:+.1}% total drift -> {}",
            self.max_drift_pct,
            if self.passed { "PASSED" } else { "FAILED" }
        );
        out
    }

    /// The window's points with their 0-based position (render helper).
    fn points_enumerated(&self) -> impl Iterator<Item = (usize, &TrendPoint)> {
        self.points.iter().enumerate()
    }
}

/// Total drift over a window, in percent: `(last - first) / first · 100`.
/// Degenerate but deterministic (the [`crate::diff`] engine's rule): a zero
/// first value with a nonzero last value is unbounded growth (`+inf`), and
/// zero-to-zero is perfectly stable.
fn drift_pct(first: f64, last: f64) -> f64 {
    if first == 0.0 {
        if last == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        (last - first) / first * 100.0
    }
}

/// Pure trend over one program's committed history: the last
/// `thresholds.window` records (append order — no clock, plan C6), the
/// first-to-last drift on each fully-covered metric, and the gate. No IO.
///
/// # Errors
///
/// [`BenchError::TrendPlan`] for a window below two records or a history
/// with fewer than two recorded runs of the program, and
/// [`BenchError::BaselineUnavailable`] when the program was never recorded.
pub fn trend_over_history(
    records: &[HistoryRecord],
    program: &str,
    thresholds: &TrendThresholds,
) -> Result<TrendReport, BenchError> {
    if thresholds.window < 2 {
        return Err(BenchError::TrendPlan {
            detail: format!(
                "the trend window is {} record(s); a trend needs at least two points",
                thresholds.window
            ),
        });
    }
    let matching: Vec<&BenchReport> = records
        .iter()
        .map(|record| &record.report)
        .filter(|report| report.program == program)
        .collect();
    if matching.is_empty() {
        return Err(BenchError::BaselineUnavailable {
            program: program.to_string(),
        });
    }
    if matching.len() < 2 {
        return Err(BenchError::TrendPlan {
            detail: format!(
                "program `{program}` has {} recorded run(s); a trend needs at \
                 least two — append more with `glyim-bench history record`",
                matching.len()
            ),
        });
    }
    let window = thresholds.window.min(matching.len());
    let windowed = &matching[matching.len() - window..];
    let points: Vec<TrendPoint> = windowed
        .iter()
        .map(|report| TrendPoint {
            wall_mean: report.wall.mean,
            instructions_mean: report.instructions.as_ref().map(|stats| stats.mean),
        })
        .collect();
    let first_wall_mean = points[0].wall_mean;
    let last_wall_mean = points[points.len() - 1].wall_mean;
    let wall_drift_pct = drift_pct(first_wall_mean, last_wall_mean);
    let instructions_drift_pct = if points.iter().all(|point| point.instructions_mean.is_some()) {
        // The all-guard above makes every point's mean available, so the
        // endpoint `?`s here are always `Some`; the `and_then` shape keeps
        // the house rule (no `expect` outside tests) and degrades to "no
        // instruction drift" (the guard's else branch) instead of panicking.
        points.first().zip(points.last()).and_then(|(first, last)| {
            Some(drift_pct(first.instructions_mean?, last.instructions_mean?))
        })
    } else {
        None
    };
    let passed = wall_drift_pct <= thresholds.max_drift_pct
        && instructions_drift_pct.is_none_or(|drift| drift <= thresholds.max_drift_pct);
    tracing::debug!(program, window, wall_drift_pct, passed, "trend complete");
    Ok(TrendReport {
        schema: BENCH_TREND_SCHEMA.to_string(),
        program: program.to_string(),
        records_total: matching.len(),
        window,
        first_wall_mean,
        last_wall_mean,
        wall_drift_pct,
        instructions_drift_pct,
        max_drift_pct: thresholds.max_drift_pct,
        points,
        passed,
    })
}

/// Loads the history through the [`HistoryStore`] seam and runs the
/// unchanged trend engine (the store-based wiring; the CLI builds the file
/// store in [`crate::cli::execute_history_trend`]).
///
/// # Errors
///
/// History load failures, and the [`trend_over_history`] plan errors.
pub fn execute_trend_from_history(
    history_store: &dyn HistoryStore,
    history_path: &Path,
    program: &str,
    thresholds: &TrendThresholds,
) -> Result<TrendReport, BenchError> {
    let _run_span = tracing::info_span!(
        "bench_trend",
        history = %history_path.display(),
        program,
    )
    .entered();
    let records = history_store.load_all(history_path)?;
    tracing::debug!(records = records.len(), "history loaded for trend");
    trend_over_history(&records, program, thresholds)
}
