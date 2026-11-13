//! The multi-report diff matrix and the geo-mean aggregate gate (plan §6
//! M2.5 remainder: "warn >1%, fail >2% per benchmark or >0.5% geo-mean",
//! restated at §325 "History & gating").
//!
//! The M2.5 diff gate compares **one** baseline/current pair; the M2.6
//! budget gate enforces **absolute** bounds per benchmark. What the plan's
//! perf-compare criterion adds — and what neither gate can express — is the
//! *aggregate* form: a change that slows every benchmark by +1% passes each
//! per-benchmark budget (+1% ≤ +2%) yet is exactly the regression class the
//! plan's "**or >0.5% geo-mean**" clause exists to catch, because a uniform
//! drift across the benchmark matrix is a real slowdown even when no single
//! benchmark trips its own budget. This module is that seam, split the way
//! [`crate::diff`] and [`crate::budget`] split:
//!
//! - [`BaselineSource`] abstracts *where the per-program baselines come
//!   from* — [`HistoryBaselineSource`] selects the **latest** recorded run
//!   of each program from a [`crate::history::HistoryStore`] (the plan's
//!   `perf/history/*.jsonl` layout), and [`InMemoryBaselineSource`] backs
//!   tests and in-process consumers. A future remote history plugs in here
//!   without the engine knowing.
//! - [`geo_mean_delta_pct`] and [`diff_matrix`] are the pure functions —
//!   no clock, no PRNG, rows in input order (plan C6). The engine performs
//!   no IO of its own: baseline lookup goes through the seam, and each row
//!   reuses the unchanged M2.5 [`crate::diff::diff_reports`] engine so a
//!   per-row verdict means exactly what the `diff` gate means.
//! - [`execute_diff_matrix`] wires the two: load every current report
//!   through the [`crate::diff::ReportStore`] seam, select baselines
//!   through [`BaselineSource`], and wrap the verdicts in a
//!   [`MatrixOutcome`] gate.
//!
//! Gate semantics extend the M2.5 gate (a metric whose mean moved beyond
//! the budget **upward** is [`crate::diff::DiffVerdict::Regressed`]; the
//! inclusive `<=` boundary convention): the matrix fails when **any** row
//! regresses, when the **wall geo-mean** exceeds its budget, or — only when
//! *every* row measured instructions — when the **instruction geo-mean**
//! exceeds its budget. The honesty rule behind the instruction geo-mean is
//! the budget gate's `Unmeasured` philosophy: a geo-mean over a *subset* of
//! the rows would look like coverage it does not have, so mixed instruction
//! coverage yields `None` and the instruction budget is not applied.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::diff::{DiffThresholds, ProgramDiff, ReportStore, diff_reports};
use crate::error::BenchError;
use crate::history::{HistoryRecord, HistoryStore, latest_baseline};
use crate::report::BenchReport;

/// Schema tag pinned for the matrix outcome shape.
pub const BENCH_MATRIX_SCHEMA: &str = "glyim.bench-matrix/1";

/// Default per-benchmark regression budget, in percent: the plan §325
/// "fail >2% on a benchmark" criterion. A CI perf job passes an explicit
/// `--max-delta-pct` when a looser envelope is the documented contract.
pub const DEFAULT_MATRIX_MAX_DELTA_PCT: f64 = 2.0;

/// Default geo-mean regression budget, in percent: the plan §325
/// "fail >0.5% geo-mean" criterion — the aggregate clause this module
/// implements.
pub const DEFAULT_MAX_GEO_DELTA_PCT: f64 = 0.5;

/// The regression budgets applied by the matrix gate: a per-benchmark mean
/// delta above `max_delta_pct` percent is a row regression, and the
/// geo-mean of the row deltas above `max_geo_delta_pct` percent fails the
/// gate even when no single row does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatrixThresholds {
    /// Allowed upward delta per benchmark, in percent.
    pub max_delta_pct: f64,
    /// Allowed upward geo-mean delta across the matrix, in percent.
    pub max_geo_delta_pct: f64,
}

impl MatrixThresholds {
    /// Budgets of `max_delta_pct` percent per benchmark and
    /// `max_geo_delta_pct` percent across the matrix (the plan's defaults
    /// are [`DEFAULT_MATRIX_MAX_DELTA_PCT`] / [`DEFAULT_MAX_GEO_DELTA_PCT`]).
    #[must_use]
    pub const fn new(max_delta_pct: f64, max_geo_delta_pct: f64) -> Self {
        Self {
            max_delta_pct,
            max_geo_delta_pct,
        }
    }
}

/// One compared row: where the current report came from and its M2.5 diff
/// against the program's selected baseline.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixRow {
    /// Where the current report was loaded from.
    pub report_path: String,
    /// The per-program diff (the unchanged M2.5 engine's verdict).
    pub diff: ProgramDiff,
}

/// The full matrix outcome: envelope, per-report rows, the aggregate
/// geo-means, and the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixOutcome {
    /// Schema tag, always [`BENCH_MATRIX_SCHEMA`].
    pub schema: String,
    /// Where the baselines were selected from (the [`BaselineSource`]'s own
    /// description, e.g. the history file with the latest-record rule).
    pub baseline_source: String,
    /// The applied per-benchmark budget, in percent.
    pub max_delta_pct: f64,
    /// The applied geo-mean budget, in percent.
    pub max_geo_delta_pct: f64,
    /// One row per input report, in input order.
    pub rows: Vec<MatrixRow>,
    /// Geo-mean of the wall deltas across all rows; `None` only for the
    /// unreachable empty-matrix case (the engine rejects an empty set).
    pub wall_geo_delta_pct: Option<f64>,
    /// Geo-mean of the instruction deltas — `Some` only when **every** row
    /// measured instructions (the honesty rule: a geo-mean over a subset is
    /// not coverage).
    pub instructions_geo_delta_pct: Option<f64>,
    /// Whether no row regressed and both measured geo-means are within
    /// their budgets.
    pub passed: bool,
}

impl MatrixOutcome {
    /// Ledger-convention alias for the gate: `true` when nothing regressed.
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

    /// Fixed-width human-readable table (deterministic for a given outcome).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "glyim-bench matrix ({})", self.schema);
        let _ = writeln!(out, "  baselines: {}", self.baseline_source);
        let _ = writeln!(
            out,
            "  budgets  : +{:.3}% per benchmark, +{:.3}% geo-mean",
            self.max_delta_pct, self.max_geo_delta_pct
        );
        for row in &self.rows {
            let _ = writeln!(out, "  program: {} ({})", row.diff.program, row.report_path);
            render_metric(&mut out, &row.diff.wall);
            if let Some(instructions) = &row.diff.instructions {
                render_metric(&mut out, instructions);
            }
        }
        render_geo(
            &mut out,
            "wall_ns",
            self.wall_geo_delta_pct,
            self.max_geo_delta_pct,
        );
        if let Some(geo) = self.instructions_geo_delta_pct {
            render_geo(&mut out, "instructions", Some(geo), self.max_geo_delta_pct);
        }
        let _ = writeln!(
            out,
            "  gate: {}",
            if self.passed { "PASSED" } else { "FAILED" }
        );
        out
    }
}

/// One metric row in the text table (the `diff` gate's rendering, reused so
/// a row reads identically in both gates).
fn render_metric(out: &mut String, metric: &crate::diff::MetricDiff) {
    let verdict = match metric.verdict {
        crate::diff::DiffVerdict::Improved => "improved",
        crate::diff::DiffVerdict::Unchanged => "unchanged",
        crate::diff::DiffVerdict::Regressed => "REGRESSED",
    };
    let _ = writeln!(
        out,
        "    {:<14}: {:.2} -> {:.2} ({:+.3}%) {}",
        metric.metric, metric.baseline, metric.current, metric.delta_pct, verdict
    );
}

/// The aggregate geo-mean line in the text table.
fn render_geo(out: &mut String, metric: &str, geo: Option<f64>, budget: f64) {
    let Some(geo) = geo else { return };
    let verdict = if geo <= budget { "within" } else { "EXCEEDED" };
    let _ = writeln!(
        out,
        "  geo-mean ({metric}): {geo:+.3}% (budget +{budget:.3}%) {verdict}"
    );
}

/// The geometric-mean aggregate of a delta series, in percent: the delta of
/// the geometric mean of the `(1 + delta)` factors. An improvement pulls
/// the aggregate down below the arithmetic mean — the property that makes
/// it the plan's uniform-drift detector (a +1% everywhere change has a
/// +1% geo-mean, while scattered noise largely cancels).
///
/// Degenerate but deterministic (the [`crate::diff::metric_diff`] rule): a
/// `-100%` factor (a current mean of zero) collapses the product to zero
/// (the aggregate reads `-100%`), and an infinite delta propagates to an
/// infinite aggregate — both verdicts fail their budget honestly.
#[must_use]
pub fn geo_mean_delta_pct(deltas_pct: &[f64]) -> f64 {
    if deltas_pct.is_empty() {
        return 0.0;
    }
    let product = deltas_pct
        .iter()
        .fold(1.0_f64, |acc, delta| acc * (1.0 + delta / 100.0));
    (product.powf(1.0 / deltas_pct.len() as f64) - 1.0) * 100.0
}

/// Where the matrix gate's per-program baselines come from.
///
/// The seam that keeps the engine IO-free and lets the baseline layout
/// evolve: today the CI shape is the plan's `perf/history/*.jsonl` store
/// ([`HistoryBaselineSource`], latest recorded run per program), tests and
/// in-process consumers use [`InMemoryBaselineSource`], and a remote or
/// database-backed history would plug in here without the engine knowing.
pub trait BaselineSource {
    /// The baseline report for `program`.
    ///
    /// # Errors
    ///
    /// [`BenchError::BaselineUnavailable`] when the source has no recorded
    /// run of the program (the gate refuses to invent a baseline), or any
    /// store error the implementation surfaces.
    fn baseline_for(&self, program: &str) -> Result<BenchReport, BenchError>;

    /// A stable description of the source for the outcome document (CI
    /// logs show where the baselines came from).
    fn describe(&self) -> String;
}

/// In-memory source: a `program -> report` map for tests and in-process
/// consumers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InMemoryBaselineSource {
    baselines: BTreeMap<String, BenchReport>,
}

impl InMemoryBaselineSource {
    /// An empty source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: register (or replace) the baseline for `report.program`.
    #[must_use]
    pub fn with_baseline(mut self, program: impl Into<String>, report: BenchReport) -> Self {
        self.baselines.insert(program.into(), report);
        self
    }

    /// Number of registered baselines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.baselines.len()
    }

    /// Whether the source holds no baselines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.baselines.is_empty()
    }
}

impl BaselineSource for InMemoryBaselineSource {
    fn baseline_for(&self, program: &str) -> Result<BenchReport, BenchError> {
        self.baselines
            .get(program)
            .cloned()
            .ok_or_else(|| BenchError::BaselineUnavailable {
                program: program.to_string(),
            })
    }

    fn describe(&self) -> String {
        format!("in-memory baselines ({} program(s))", self.baselines.len())
    }
}

/// The history-backed source: the **latest** recorded run of each program
/// in one [`crate::history::HistoryStore`] file (plan §6 M2.5: file order
/// is append order, so the choice is deterministic without a clock).
///
/// The history is loaded lazily on the first lookup and cached — one file
/// read per matrix, not per program.
pub struct HistoryBaselineSource<'a> {
    store: &'a dyn HistoryStore,
    path: PathBuf,
    records: RefCell<Option<Vec<HistoryRecord>>>,
}

impl<'a> HistoryBaselineSource<'a> {
    /// A source over `store` at `path` (the conventional layout is one
    /// JSONL file covering every benchmark in the matrix).
    #[must_use]
    pub fn new(store: &'a dyn HistoryStore, path: &Path) -> Self {
        Self {
            store,
            path: path.to_path_buf(),
            records: RefCell::new(None),
        }
    }

    /// Loads (or returns the cached) history records.
    ///
    /// # Errors
    ///
    /// [`BenchError::HistoryRead`] / [`BenchError::HistoryParse`] from the
    /// underlying store.
    fn load_records(&self) -> Result<Vec<HistoryRecord>, BenchError> {
        let mut cached = self.records.borrow_mut();
        if let Some(records) = cached.as_ref() {
            return Ok(records.clone());
        }
        let records = self.store.load_all(&self.path)?;
        *cached = Some(records.clone());
        Ok(records)
    }
}

impl BaselineSource for HistoryBaselineSource<'_> {
    fn baseline_for(&self, program: &str) -> Result<BenchReport, BenchError> {
        latest_baseline(&self.load_records()?, program)
    }

    fn describe(&self) -> String {
        format!(
            "history {} (latest recorded run per program)",
            self.path.display()
        )
    }
}

/// Pure matrix diff of a current-report set against per-program baselines:
/// per-row verdicts via the unchanged M2.5 engine, plus the geo-mean
/// aggregates. No IO of its own — baseline lookup goes through the
/// [`BaselineSource`] seam. Rows come in input order (plan C6).
///
/// # Errors
///
/// [`BenchError::MatrixPlan`] for an empty report set or a program measured
/// twice in one matrix, [`BenchError::BaselineUnavailable`] when a program
/// has no baseline, and [`BenchError::InvalidPlan`] when a baseline and its
/// current report disagree on the program (the M2.5 engine's guard).
pub fn diff_matrix(
    reports: &[(PathBuf, BenchReport)],
    baselines: &dyn BaselineSource,
    thresholds: &MatrixThresholds,
) -> Result<MatrixOutcome, BenchError> {
    if reports.is_empty() {
        return Err(BenchError::MatrixPlan {
            detail: "the matrix has no reports; pass each benchmark's \
                     `glyim.bench/1` report exactly once"
                .to_string(),
        });
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(reports.len());
    for (path, report) in reports {
        if !seen.insert(report.program.as_str()) {
            return Err(BenchError::MatrixPlan {
                detail: format!(
                    "program `{}` appears twice in the matrix; a row must be unique \
                     per program or the geo-mean double-counts it",
                    report.program
                ),
            });
        }
        let baseline = baselines.baseline_for(&report.program)?;
        let diff = diff_reports(
            &baseline,
            report,
            &DiffThresholds::new(thresholds.max_delta_pct),
        )?;
        rows.push(MatrixRow {
            report_path: path.display().to_string(),
            diff,
        });
    }
    let wall_geo = geo_mean_delta_pct(
        &rows
            .iter()
            .map(|row| row.diff.wall.delta_pct)
            .collect::<Vec<_>>(),
    );
    let instruction_deltas: Vec<f64> = rows
        .iter()
        .filter_map(|row| row.diff.instructions.as_ref().map(|i| i.delta_pct))
        .collect();
    let instructions_geo = if instruction_deltas.len() == rows.len() {
        Some(geo_mean_delta_pct(&instruction_deltas))
    } else {
        None
    };
    let per_row_passed = rows.iter().all(|row| {
        row.diff.wall.verdict != crate::diff::DiffVerdict::Regressed
            && row
                .diff
                .instructions
                .as_ref()
                .is_none_or(|i| i.verdict != crate::diff::DiffVerdict::Regressed)
    });
    let geo_passed = wall_geo <= thresholds.max_geo_delta_pct
        && instructions_geo.is_none_or(|geo| geo <= thresholds.max_geo_delta_pct);
    let passed = per_row_passed && geo_passed;
    Ok(MatrixOutcome {
        schema: BENCH_MATRIX_SCHEMA.to_string(),
        baseline_source: baselines.describe(),
        max_delta_pct: thresholds.max_delta_pct,
        max_geo_delta_pct: thresholds.max_geo_delta_pct,
        wall_geo_delta_pct: Some(wall_geo),
        instructions_geo_delta_pct: instructions_geo,
        rows,
        passed,
    })
}

/// Loads every current report through the [`ReportStore`] seam, selects the
/// baselines through the [`BaselineSource`] seam, and wraps the gate (the
/// store-based wiring; the CLI builds the file/History stores in
/// [`crate::cli::execute_matrix`]).
///
/// # Errors
///
/// Report load failures, [`BenchError::BaselineUnavailable`] for a program
/// with no recorded baseline, and the [`diff_matrix`] plan errors.
pub fn execute_diff_matrix(
    report_store: &dyn ReportStore,
    baselines: &dyn BaselineSource,
    current_paths: &[PathBuf],
    thresholds: &MatrixThresholds,
) -> Result<MatrixOutcome, BenchError> {
    let _run_span = tracing::info_span!(
        "bench_diff_matrix",
        baselines = %baselines.describe(),
        reports = current_paths.len(),
    )
    .entered();
    let mut reports = Vec::with_capacity(current_paths.len());
    for path in current_paths {
        let report = report_store.load(path)?;
        tracing::debug!(report = %path.display(), program = %report.program, "matrix report loaded");
        reports.push((path.clone(), report));
    }
    let outcome = diff_matrix(&reports, baselines, thresholds)?;
    tracing::debug!(
        rows = outcome.rows.len(),
        passed = outcome.passed,
        "matrix diff complete"
    );
    Ok(outcome)
}
