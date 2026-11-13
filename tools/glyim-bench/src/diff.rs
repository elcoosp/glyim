//! Report-to-report comparison (plan M2.5, the perf-history seam).
//!
//! `report.rs` pinned the contract that two `glyim.bench/1` documents are
//! byte-deterministic "which is what the M2.5 perf-history store diffs" —
//! but no diff existed. This module is that seam, split the way the ledger
//! separates input sources from pure computation:
//!
//! - [`ReportStore`] abstracts *where reports come from / go*. Today the
//!   CLI uses [`FileReportStore`] (a `glyim.bench/1` document on disk);
//!   [`InMemoryReportStore`] backs tests and in-process consumers. The
//!   M2.5 perf-history store plugs in here without touching the engine.
//! - [`diff_reports`] is the pure function `(baseline, current, budget) →
//!   per-metric deltas and verdicts` — no clock, no randomness, sorted
//!   output (plan C6).
//! - [`execute_diff`] wires the two: load both sides through the store,
//!   diff, and wrap in a [`BenchDiffOutcome`] with the regression gate.
//!
//! Gate semantics mirror the M2.2 stability gate: a metric whose mean
//! moved beyond the budget **upward** is [`DiffVerdict::Regressed`]; the
//! outcome fails (`is_ok() == false`) when any metric regressed. Improvements
//! are reported as [`DiffVerdict::Improved`] and never fail the gate.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::BenchError;
use crate::report::BenchReport;

/// Schema tag pinned for the diff outcome shape.
pub const BENCH_DIFF_SCHEMA: &str = "glyim.bench-diff/1";

/// The regression budget applied per metric: a mean delta above
/// `max_delta_pct` percent (relative to the baseline) is a regression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffThresholds {
    /// Allowed upward delta, in percent.
    pub max_delta_pct: f64,
}

impl DiffThresholds {
    /// A budget of `max_delta_pct` percent (e.g. `5.0` allows a 5 % mean
    /// slowdown before a metric regresses).
    #[must_use]
    pub const fn new(max_delta_pct: f64) -> Self {
        Self { max_delta_pct }
    }
}

/// Verdict for one compared metric, relative to the baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DiffVerdict {
    /// The current mean is strictly below the baseline.
    Improved,
    /// The current mean is within the budget of the baseline.
    Unchanged,
    /// The current mean exceeds the baseline by more than the budget.
    Regressed,
}

/// Delta and verdict for one metric (a report's `wall` mean, or its
/// optional `instructions` mean).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDiff {
    /// Which metric this row compares (`wall_ns` or `instructions`).
    pub metric: String,
    /// Baseline mean.
    pub baseline: f64,
    /// Current mean.
    pub current: f64,
    /// Relative change, in percent (`(current - baseline) / baseline`).
    pub delta_pct: f64,
    /// The budget verdict for this metric.
    pub verdict: DiffVerdict,
}

/// Per-program comparison of one baseline/current report pair.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDiff {
    /// The measured program (from the reports; both sides must agree).
    pub program: String,
    /// Wall-clock comparison (always present — every source measures it).
    pub wall: MetricDiff,
    /// Instruction-count comparison, present only when **both** reports
    /// carry the M1.5 `instructions` summary.
    pub instructions: Option<MetricDiff>,
}

/// The full diff outcome: envelope, per-program rows, and the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchDiffOutcome {
    /// Schema tag, always [`BENCH_DIFF_SCHEMA`].
    pub schema: String,
    /// Where the baseline report was loaded from.
    pub baseline_path: String,
    /// Where the current report was loaded from.
    pub current_path: String,
    /// The applied regression budget, in percent.
    pub max_delta_pct: f64,
    /// One row per compared program (currently exactly one).
    pub diffs: Vec<ProgramDiff>,
    /// Whether no metric regressed beyond the budget.
    pub passed: bool,
}

impl BenchDiffOutcome {
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
        let _ = writeln!(out, "glyim-bench diff ({})", self.schema);
        let _ = writeln!(out, "  baseline: {}", self.baseline_path);
        let _ = writeln!(out, "  current : {}", self.current_path);
        let _ = writeln!(
            out,
            "  budget  : +{:.3}% (means above this delta regress)",
            self.max_delta_pct
        );
        for diff in &self.diffs {
            let _ = writeln!(out, "  program: {}", diff.program);
            render_metric(&mut out, &diff.wall);
            if let Some(instructions) = &diff.instructions {
                render_metric(&mut out, instructions);
            }
        }
        let _ = writeln!(
            out,
            "  gate: {}",
            if self.passed { "PASSED" } else { "FAILED" }
        );
        out
    }
}

/// One metric row in the text table.
fn render_metric(out: &mut String, metric: &MetricDiff) {
    let verdict = match metric.verdict {
        DiffVerdict::Improved => "improved",
        DiffVerdict::Unchanged => "unchanged",
        DiffVerdict::Regressed => "REGRESSED",
    };
    let _ = writeln!(
        out,
        "    {:<14}: {:.2} -> {:.2} ({:+.3}%) {}",
        metric.metric, metric.baseline, metric.current, metric.delta_pct, verdict
    );
}

/// Compares one metric's summary stats and applies the budget.
fn metric_diff(
    metric: &str,
    baseline: f64,
    current: f64,
    thresholds: &DiffThresholds,
) -> MetricDiff {
    let delta_pct = if baseline == 0.0 {
        // Degenerate but deterministic: zero -> nonzero is unbounded growth.
        if current == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        (current - baseline) / baseline * 100.0
    };
    let verdict = if delta_pct > thresholds.max_delta_pct {
        DiffVerdict::Regressed
    } else if delta_pct < -1e-9 {
        DiffVerdict::Improved
    } else {
        DiffVerdict::Unchanged
    };
    MetricDiff {
        metric: metric.to_string(),
        baseline,
        current,
        delta_pct,
        verdict,
    }
}

/// Pure diff of two reports over the same program: per-metric deltas and
/// verdicts, no IO.
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] when the reports measure different programs
/// (a diff across programs would be a spurious verdict).
pub fn diff_reports(
    baseline: &BenchReport,
    current: &BenchReport,
    thresholds: &DiffThresholds,
) -> Result<ProgramDiff, BenchError> {
    if baseline.program != current.program {
        return Err(BenchError::InvalidPlan {
            detail: format!(
                "baseline measures `{}` but current measures `{}`; a diff must compare the same program",
                baseline.program, current.program
            ),
        });
    }
    let wall = metric_diff("wall_ns", baseline.wall.mean, current.wall.mean, thresholds);
    let instructions = match (
        baseline.instructions.as_ref(),
        current.instructions.as_ref(),
    ) {
        (Some(base), Some(cur)) => {
            Some(metric_diff("instructions", base.mean, cur.mean, thresholds))
        }
        _ => None,
    };
    Ok(ProgramDiff {
        program: baseline.program.clone(),
        wall,
        instructions,
    })
}

/// Where `glyim.bench/1` reports are loaded from and persisted to.
///
/// The seam that lets the M2.5 perf-history store provide baselines (a
/// committed file tree today, a database or remote history later) without
/// the diff engine knowing. Both methods take `&self`; implementations
/// own their interior mutability.
pub trait ReportStore {
    /// Load one `glyim.bench/1` report.
    ///
    /// # Errors
    ///
    /// [`BenchError::ReportRead`] / [`BenchError::ReportParse`] naming the
    /// failing path.
    fn load(&self, path: &Path) -> Result<BenchReport, BenchError>;

    /// Persist one report (the diff CLI's counterpart to `run --json`).
    ///
    /// # Errors
    ///
    /// [`BenchError::ReportWrite`] with the failing path, or
    /// [`BenchError::ReportParse`] if serialisation fails.
    fn save(&self, path: &Path, report: &BenchReport) -> Result<(), BenchError>;
}

/// The default store: `glyim.bench/1` documents on the local filesystem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileReportStore;

impl ReportStore for FileReportStore {
    fn load(&self, path: &Path) -> Result<BenchReport, BenchError> {
        let text = std::fs::read_to_string(path).map_err(|err| BenchError::ReportRead {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
        serde_json::from_str(&text).map_err(|err| BenchError::ReportParse {
            detail: err.to_string(),
        })
    }

    fn save(&self, path: &Path, report: &BenchReport) -> Result<(), BenchError> {
        let text = report.to_json_pretty()?;
        std::fs::write(path, text).map_err(|err| BenchError::ReportWrite {
            path: path.display().to_string(),
            detail: err.to_string(),
        })
    }
}

/// In-memory store: a `PathBuf -> report` map for tests and in-process
/// consumers (and as a fixture source for the perf-history store's tests).
#[derive(Debug, Default)]
pub struct InMemoryReportStore {
    reports: RefCell<BTreeMap<PathBuf, BenchReport>>,
}

impl InMemoryReportStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored reports.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reports.borrow().len()
    }

    /// Whether the store holds no reports.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reports.borrow().is_empty()
    }
}

impl ReportStore for InMemoryReportStore {
    fn load(&self, path: &Path) -> Result<BenchReport, BenchError> {
        self.reports
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| BenchError::ReportRead {
                path: path.display().to_string(),
                detail: "not found in the in-memory store".to_string(),
            })
    }

    fn save(&self, path: &Path, report: &BenchReport) -> Result<(), BenchError> {
        let _ = report.to_json_pretty()?; // same serialisation contract as disk
        self.reports
            .borrow_mut()
            .insert(path.to_path_buf(), report.clone());
        Ok(())
    }
}

/// Load both sides through `store`, diff them, and wrap the gate.
///
/// # Errors
///
/// Store load failures, or [`BenchError::InvalidPlan`] on a program
/// mismatch between the two reports.
pub fn execute_diff(
    store: &dyn ReportStore,
    baseline_path: &Path,
    current_path: &Path,
    thresholds: &DiffThresholds,
) -> Result<BenchDiffOutcome, BenchError> {
    let baseline = store.load(baseline_path)?;
    let current = store.load(current_path)?;
    let diff = diff_reports(&baseline, &current, thresholds)?;
    let passed = diff.wall.verdict != DiffVerdict::Regressed
        && diff
            .instructions
            .as_ref()
            .is_none_or(|i| i.verdict != DiffVerdict::Regressed);
    Ok(BenchDiffOutcome {
        schema: BENCH_DIFF_SCHEMA.to_string(),
        baseline_path: baseline_path.display().to_string(),
        current_path: current_path.display().to_string(),
        max_delta_pct: thresholds.max_delta_pct,
        diffs: vec![diff],
        passed,
    })
}
