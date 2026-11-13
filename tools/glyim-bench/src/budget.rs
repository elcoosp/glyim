//! Absolute per-program budget enforcement (plan M2.6, the budgets seam).
//!
//! The M2.5 diff gate is *relative*: it compares a current report against a
//! baseline report. Constitution C7's other half — "budgets are tests" —
//! needs the *absolute* form: a committed `perf/budgets.toml` per benchmark,
//! so a benchmark without a budget entry cannot pass CI at all (plan §6
//! M2.6: "initial `perf/budgets.toml` = baseline; budgets enforced"). This
//! module is that seam, split exactly like [`crate::diff`]:
//!
//! - [`BudgetStore`] abstracts *where the budgets file comes from*
//!   ([`FileBudgetStore`] reads the committed `glyim.bench-budgets/1` TOML
//!   document; [`InMemoryBudgetStore`] backs tests and in-process
//!   consumers).
//! - [`check_budgets`] is the pure function `(reports, budgets) →
//!   per-metric verdicts` — no clock, no IO, input-order output (plan C6).
//! - [`execute_budget`] wires the two: load the budgets through the store,
//!   load every report through the [`crate::diff::ReportStore`] seam, and
//!   wrap the verdicts in a [`BudgetOutcome`] gate.
//!
//! Verdict semantics mirror the M2.2 stability gate's inclusive `<=`
//! boundary ([`BudgetVerdict::WithinBudget`] when `actual <= budget`), and
//! extend the honesty rule behind the ledger's gap caps: a metric the
//! report does not carry is [`BudgetVerdict::Unmeasured`] (a wall-only run
//! cannot silently satisfy an instruction budget), and a program with no
//! budget entry is an outright gate failure — a benchmark that never
//! declared a budget cannot be "within" one.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::BenchError;
use crate::report::BenchReport;

/// Schema tag pinned for the budgets file shape.
pub const BENCH_BUDGETS_SCHEMA: &str = "glyim.bench-budgets/1";

/// Schema tag pinned for the budget-gate outcome shape.
pub const BENCH_BUDGET_SCHEMA: &str = "glyim.bench-budget/1";

/// The absolute budget for one benchmark program.
///
/// Every bound is optional, but an entry with *no* bound is rejected at
/// load: a budget row that bounds nothing cannot fail, so it would be
/// enforcement theatre.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetEntry {
    /// The measured program this row budgets (the report's `program`).
    pub program: String,
    /// Maximum accepted wall-clock mean, in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_wall_mean_ns: Option<u64>,
    /// Maximum accepted instruction-count mean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_instructions_mean: Option<u64>,
}

/// The committed budgets document (`glyim.bench-budgets/1`, TOML on disk).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetFile {
    /// Schema tag, always [`BENCH_BUDGETS_SCHEMA`].
    pub schema: String,
    /// One row per budgeted program; programs are unique.
    #[serde(rename = "budget", default)]
    pub entries: Vec<BudgetEntry>,
}

impl BudgetFile {
    /// Parses and validates a `glyim.bench-budgets/1` TOML document.
    ///
    /// # Errors
    ///
    /// [`BenchError::BudgetsParse`] for a malformed document, a wrong
    /// schema tag, duplicate program rows, or a row without any bound.
    pub fn from_toml_str(text: &str) -> Result<Self, BenchError> {
        let file: Self = toml::from_str(text).map_err(|err| BenchError::BudgetsParse {
            detail: err.to_string(),
        })?;
        file.validated()
    }

    /// Enforces the non-derivable contract: unique programs, no boundless
    /// rows.
    ///
    /// # Errors
    ///
    /// [`BenchError::BudgetsParse`] with the offending program named.
    pub fn validated(self) -> Result<Self, BenchError> {
        if self.schema != BENCH_BUDGETS_SCHEMA {
            return Err(BenchError::BudgetsParse {
                detail: format!(
                    "schema tag is `{}` but budgets files are `{BENCH_BUDGETS_SCHEMA}` documents",
                    self.schema
                ),
            });
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            if !seen.insert(entry.program.as_str()) {
                return Err(BenchError::BudgetsParse {
                    detail: format!(
                        "duplicate budget for program `{}`; a program must have exactly one row",
                        entry.program
                    ),
                });
            }
            if entry.max_wall_mean_ns.is_none() && entry.max_instructions_mean.is_none() {
                return Err(BenchError::BudgetsParse {
                    detail: format!(
                        "budget for program `{}` sets neither `max_wall_mean_ns` nor \
                         `max_instructions_mean`; a row that bounds nothing cannot fail",
                        entry.program
                    ),
                });
            }
        }
        Ok(self)
    }
}

/// Verdict for one budgeted metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BudgetVerdict {
    /// The actual mean is within the budget (`actual <= budget`, the M2.2
    /// gate's inclusive boundary).
    WithinBudget,
    /// The actual mean is strictly above the budget.
    Exceeded,
    /// The budget sets a bound for a metric the report does not carry.
    Unmeasured,
}

/// One compared metric: the committed bound, the measured mean, the verdict.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCheckRow {
    /// Which metric this row checks (`wall_ns` or `instructions`).
    pub metric: String,
    /// The committed absolute budget.
    pub budget: u64,
    /// The measured mean; `None` when the report does not carry the metric
    /// (the verdict is then [`BudgetVerdict::Unmeasured`]).
    pub actual: Option<f64>,
    /// The budget verdict for this metric.
    pub verdict: BudgetVerdict,
}

/// Per-report budget check: identity, coverage, and the metric rows.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramBudget {
    /// Where the checked report was loaded from.
    pub report_path: String,
    /// The measured program (from the report).
    pub program: String,
    /// Whether the budgets file has a row for this program; a report whose
    /// program is unbudgeted fails the gate (a benchmark without a budget
    /// entry cannot pass CI).
    pub budgeted: bool,
    /// One row per bound set on the entry (empty when unbudgeted).
    pub rows: Vec<BudgetCheckRow>,
}

/// The full budget-gate outcome: envelope, per-report checks, and the gate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetOutcome {
    /// Schema tag, always [`BENCH_BUDGET_SCHEMA`].
    pub schema: String,
    /// Where the budgets file was loaded from.
    pub budgets_path: String,
    /// One check per input report, in input order.
    pub checks: Vec<ProgramBudget>,
    /// Whether every report was budgeted and within every bound.
    pub passed: bool,
}

impl BudgetOutcome {
    /// Ledger-convention alias for the gate: `true` when every check passed.
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
        let _ = writeln!(out, "glyim-bench budget ({})", self.schema);
        let _ = writeln!(out, "  budgets: {}", self.budgets_path);
        for check in &self.checks {
            let _ = writeln!(out, "  program: {} ({})", check.program, check.report_path);
            if !check.budgeted {
                let _ = writeln!(
                    out,
                    "    no budget entry; a benchmark without a budget cannot pass"
                );
                continue;
            }
            for row in &check.rows {
                let verdict = match row.verdict {
                    BudgetVerdict::WithinBudget => "within",
                    BudgetVerdict::Exceeded => "EXCEEDED",
                    BudgetVerdict::Unmeasured => "UNMEASURED",
                };
                let actual = row
                    .actual
                    .map_or_else(|| "-".to_string(), |v| format!("{v:.2}"));
                let _ = writeln!(
                    out,
                    "    {:<14}: mean {} / budget {} {}",
                    row.metric, actual, row.budget, verdict
                );
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

/// Verdicts one metric: `None` actual is [`BudgetVerdict::Unmeasured`];
/// otherwise the M2.2 gate's inclusive `actual <= budget` boundary.
fn metric_verdict(budget: u64, actual: Option<f64>) -> BudgetVerdict {
    match actual {
        None => BudgetVerdict::Unmeasured,
        Some(actual) if actual <= budget as f64 => BudgetVerdict::WithinBudget,
        Some(_) => BudgetVerdict::Exceeded,
    }
}

/// Builds the rows for one report against its (already matched) entry.
fn check_rows(entry: &BudgetEntry, report: &BenchReport) -> Vec<BudgetCheckRow> {
    let mut rows = Vec::new();
    if let Some(budget) = entry.max_wall_mean_ns {
        rows.push(BudgetCheckRow {
            metric: "wall_ns".to_string(),
            budget,
            actual: Some(report.wall.mean),
            verdict: metric_verdict(budget, Some(report.wall.mean)),
        });
    }
    if let Some(budget) = entry.max_instructions_mean {
        let actual = report.instructions.as_ref().map(|stats| stats.mean);
        rows.push(BudgetCheckRow {
            metric: "instructions".to_string(),
            budget,
            actual,
            verdict: metric_verdict(budget, actual),
        });
    }
    rows
}

/// Pure budget check of a report set against a budgets file: per-metric
/// verdicts, no IO, output in input order (plan C6).
#[must_use]
pub fn check_budgets(reports: &[(PathBuf, BenchReport)], budgets: &BudgetFile) -> BudgetOutcome {
    let mut checks = Vec::with_capacity(reports.len());
    for (path, report) in reports {
        let entry = budgets
            .entries
            .iter()
            .find(|entry| entry.program == report.program);
        let (budgeted, rows) = match entry {
            Some(entry) => (true, check_rows(entry, report)),
            None => (false, Vec::new()),
        };
        checks.push(ProgramBudget {
            report_path: path.display().to_string(),
            program: report.program.clone(),
            budgeted,
            rows,
        });
    }
    let passed = checks.iter().all(|check| {
        check.budgeted
            && check
                .rows
                .iter()
                .all(|row| row.verdict == BudgetVerdict::WithinBudget)
    });
    BudgetOutcome {
        schema: BENCH_BUDGET_SCHEMA.to_string(),
        budgets_path: String::new(),
        checks,
        passed,
    }
}

/// Where `glyim.bench-budgets/1` documents are loaded from and persisted
/// to. The budgets file is the committed *input* contract — hand-edited,
/// review-gated, never generated from a measurement — so `save` exists for
/// the in-memory seam's symmetry and for tooling that seeds a starter file,
/// not for the gate itself.
pub trait BudgetStore {
    /// Load one `glyim.bench-budgets/1` document (fully validated).
    ///
    /// # Errors
    ///
    /// [`BenchError::BudgetsRead`] / [`BenchError::BudgetsParse`] naming the
    /// failing path.
    fn load(&self, path: &Path) -> Result<BudgetFile, BenchError>;

    /// Persist one budgets document (validated on the way out as well).
    ///
    /// # Errors
    ///
    /// [`BenchError::BudgetsParse`] with the failing path, or
    /// [`BenchError::BudgetsRead`] when the write fails.
    fn save(&self, path: &Path, budgets: &BudgetFile) -> Result<(), BenchError>;
}

/// The default store: `glyim.bench-budgets/1` TOML documents on the local
/// filesystem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileBudgetStore;

impl BudgetStore for FileBudgetStore {
    fn load(&self, path: &Path) -> Result<BudgetFile, BenchError> {
        let text = std::fs::read_to_string(path).map_err(|err| BenchError::BudgetsRead {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
        BudgetFile::from_toml_str(&text).map_err(|err| match err {
            // Name the offending file, not just the contract.
            BenchError::BudgetsParse { detail } => BenchError::BudgetsParse {
                detail: format!("{}: {detail}", path.display()),
            },
            other => other,
        })
    }

    fn save(&self, path: &Path, budgets: &BudgetFile) -> Result<(), BenchError> {
        let budgets = budgets.clone().validated()?;
        let text = toml::to_string_pretty(&budgets).map_err(|err| BenchError::BudgetsParse {
            detail: err.to_string(),
        })?;
        std::fs::write(path, text).map_err(|err| BenchError::BudgetsRead {
            path: path.display().to_string(),
            detail: err.to_string(),
        })
    }
}

/// In-memory store: a `PathBuf -> budgets` map for tests and in-process
/// consumers.
#[derive(Debug, Default)]
pub struct InMemoryBudgetStore {
    budgets: RefCell<BTreeMap<PathBuf, BudgetFile>>,
}

impl InMemoryBudgetStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl BudgetStore for InMemoryBudgetStore {
    fn load(&self, path: &Path) -> Result<BudgetFile, BenchError> {
        self.budgets
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| BenchError::BudgetsRead {
                path: path.display().to_string(),
                detail: "not found in the in-memory store".to_string(),
            })
    }

    fn save(&self, path: &Path, budgets: &BudgetFile) -> Result<(), BenchError> {
        let budgets = budgets.clone().validated()?;
        self.budgets
            .borrow_mut()
            .insert(path.to_path_buf(), budgets);
        Ok(())
    }
}

/// Loads the budgets and every report through their stores and wraps the
/// gate (the store-based wiring; the CLI builds the file stores in
/// [`crate::cli::execute_budget`]).
///
/// # Errors
///
/// Budgets-file or report load failures propagate untouched.
pub fn execute_budget(
    budgets_store: &dyn BudgetStore,
    report_store: &dyn crate::diff::ReportStore,
    budgets_path: &Path,
    report_paths: &[PathBuf],
) -> Result<BudgetOutcome, BenchError> {
    let _run_span = tracing::info_span!(
        "bench_budget",
        budgets = %budgets_path.display(),
        reports = report_paths.len(),
    )
    .entered();
    let budgets = budgets_store.load(budgets_path)?;
    let mut reports = Vec::with_capacity(report_paths.len());
    for path in report_paths {
        let report = report_store.load(path)?;
        tracing::debug!(report = %path.display(), program = %report.program, "budget check loaded");
        reports.push((path.clone(), report));
    }
    let mut outcome = check_budgets(&reports, &budgets);
    outcome.budgets_path = budgets_path.display().to_string();
    tracing::debug!(passed = outcome.passed, "budget gate complete");
    Ok(outcome)
}
