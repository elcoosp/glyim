//! The perf-history store (plan §6 M2.5: "`perf/history/*.jsonl` appended
//! on `main`").
//!
//! The M2.5 diff gate compares two explicit report paths; what the plan's
//! history store adds is the *committed memory*: every measurement run on
//! `main` appends its `glyim.bench/1` report to a JSONL history file, and a
//! PR's diff gate selects its baseline as the **latest** record for the
//! measured program. This module is that seam, split exactly like
//! [`crate::diff`] and [`crate::budget`]:
//!
//! - [`HistoryStore`] abstracts *where history lives* — [`JsonlHistoryStore`]
//!   (one self-describing `glyim.bench-history/1` JSON object per line,
//!   append-only) and [`InMemoryHistoryStore`] for tests and in-process
//!   consumers. The store is path-agnostic: the CI layout (one file per
//!   benchmark under `perf/history/`) is a caller decision;
//!   [`program_slug`] derives the conventional filesystem-safe name.
//! - [`latest_baseline`] is the pure selection: the last record whose
//!   report measures the program — file order is append order, so the
//!   choice is deterministic without a clock (plan C6).
//! - [`execute_diff_from_history`] wires the stores: current report through
//!   the [`crate::diff::ReportStore`] seam, baseline through the history,
//!   then the unchanged M2.5 diff engine and gate.
//!
//! A history file is *not* a report file: each line carries its own schema
//! tag (`glyim.bench-history/1`) so a history accidentally handed to
//! `diff` fails loudly instead of comparing against a wrapper.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diff::{BenchDiffOutcome, DiffThresholds, DiffVerdict, ReportStore, diff_reports};
use crate::error::BenchError;
use crate::report::BenchReport;

/// Schema tag pinned for the history record shape (one JSONL line).
pub const HISTORY_SCHEMA: &str = "glyim.bench-history/1";

/// One committed history entry: a finished `glyim.bench/1` report plus the
/// history schema tag.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRecord {
    /// Schema tag, always [`HISTORY_SCHEMA`].
    pub schema: String,
    /// The recorded report, byte-identical to what `run --json` wrote.
    pub report: BenchReport,
}

impl HistoryRecord {
    /// Wraps a finished report as a history record.
    #[must_use]
    pub fn new(report: BenchReport) -> Self {
        Self {
            schema: HISTORY_SCHEMA.to_string(),
            report,
        }
    }

    /// One compact JSON line (deterministic field order via the struct
    /// shape; a history file is one record per line).
    ///
    /// # Errors
    ///
    /// [`BenchError::HistoryParse`] if serialisation fails (cannot happen
    /// for in-memory values, but kept for the `Result`-shaped seam).
    pub fn to_jsonl_line(&self) -> Result<String, BenchError> {
        serde_json::to_string(self).map_err(|err| BenchError::HistoryParse {
            detail: err.to_string(),
        })
    }

    /// Parses one history line.
    ///
    /// # Errors
    ///
    /// [`BenchError::HistoryParse`] for malformed JSON or a wrong schema
    /// tag.
    pub fn from_jsonl_line(line: &str) -> Result<Self, BenchError> {
        let record: Self = serde_json::from_str(line).map_err(|err| BenchError::HistoryParse {
            detail: err.to_string(),
        })?;
        if record.schema != HISTORY_SCHEMA {
            return Err(BenchError::HistoryParse {
                detail: format!(
                    "schema tag is `{}` but history records are `{HISTORY_SCHEMA}`",
                    record.schema
                ),
            });
        }
        Ok(record)
    }
}

/// Derives the conventional per-benchmark history filename stem from a
/// program name: runs of non-alphanumeric characters collapse to one `-`.
///
/// Deterministic and filesystem-safe; collisions across distinct program
/// names are the caller's layout concern (the CI layout keys files by the
/// benchmark list, which is review-gated).
#[must_use]
pub fn program_slug(program: &str) -> String {
    let mut slug = String::with_capacity(program.len());
    let mut last_dash = true; // suppress a leading dash
    for ch in program.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// The pure baseline selection: the **last** record whose report measures
/// `program` (file order is append order — no clock involved, plan C6).
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] when no recorded report measures the
/// program (an empty history is an uncomparable gate, not a pass).
pub fn latest_baseline(
    records: &[HistoryRecord],
    program: &str,
) -> Result<BenchReport, BenchError> {
    records
        .iter()
        .rev()
        .map(|record| &record.report)
        .find(|report| report.program == program)
        .cloned()
        .ok_or_else(|| BenchError::InvalidPlan {
            detail: format!(
                "no recorded history for program `{program}`; record a baseline with \
                 `glyim-bench history record` before diffing from history"
            ),
        })
}

/// Where `glyim.bench-history/1` JSONL files are appended to and loaded
/// from.
pub trait HistoryStore {
    /// Appends one record (creating the file on first append).
    ///
    /// # Errors
    ///
    /// [`BenchError::HistoryRead`] when the write fails, or
    /// [`BenchError::HistoryParse`] if serialisation fails.
    fn append(&self, path: &Path, record: &HistoryRecord) -> Result<(), BenchError>;

    /// Loads every record, in append order. A missing file is an empty
    /// history (the first `append` creates it); a broken line is an error
    /// naming the line.
    ///
    /// # Errors
    ///
    /// [`BenchError::HistoryRead`] when the read fails, or
    /// [`BenchError::HistoryParse`] for a malformed line.
    fn load_all(&self, path: &Path) -> Result<Vec<HistoryRecord>, BenchError>;
}

/// The default store: append-only JSONL on the local filesystem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonlHistoryStore;

impl HistoryStore for JsonlHistoryStore {
    fn append(&self, path: &Path, record: &HistoryRecord) -> Result<(), BenchError> {
        use std::io::Write as _;
        let line = record.to_jsonl_line()?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| BenchError::HistoryRead {
                path: path.display().to_string(),
                detail: err.to_string(),
            })?;
        writeln!(file, "{line}").map_err(|err| BenchError::HistoryRead {
            path: path.display().to_string(),
            detail: err.to_string(),
        })
    }

    fn load_all(&self, path: &Path) -> Result<Vec<HistoryRecord>, BenchError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            // A missing file is an empty history: the first append creates
            // it, and `latest_baseline` turns "nothing recorded" into the
            // honest gate error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(BenchError::HistoryRead {
                    path: path.display().to_string(),
                    detail: err.to_string(),
                });
            }
        };
        let mut records = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(
                HistoryRecord::from_jsonl_line(line).map_err(|err| match err {
                    BenchError::HistoryParse { detail } => BenchError::HistoryParse {
                        detail: format!("{} line {}: {detail}", path.display(), index + 1),
                    },
                    other => other,
                })?,
            );
        }
        Ok(records)
    }
}

/// In-memory store: a `PathBuf -> records` map for tests and in-process
/// consumers.
#[derive(Debug, Default)]
pub struct InMemoryHistoryStore {
    records: RefCell<BTreeMap<PathBuf, Vec<HistoryRecord>>>,
}

impl InMemoryHistoryStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl HistoryStore for InMemoryHistoryStore {
    fn append(&self, path: &Path, record: &HistoryRecord) -> Result<(), BenchError> {
        let _ = record.to_jsonl_line()?; // same serialisation contract as disk
        self.records
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_default()
            .push(record.clone());
        Ok(())
    }

    fn load_all(&self, path: &Path) -> Result<Vec<HistoryRecord>, BenchError> {
        Ok(self.records.borrow().get(path).cloned().unwrap_or_default())
    }
}

/// Loads the current report through the [`ReportStore`] seam and the
/// baseline through the [`HistoryStore`] seam, then runs the unchanged M2.5
/// diff engine and gate. The outcome's `baseline_path` names the history
/// file with the `#latest` selector so CI logs show where the baseline
/// came from.
///
/// # Errors
///
/// Store load failures, or [`BenchError::InvalidPlan`] when no recorded
/// history measures the current report's program.
pub fn execute_diff_from_history(
    report_store: &dyn ReportStore,
    history_store: &dyn HistoryStore,
    history_path: &Path,
    current_path: &Path,
    thresholds: &DiffThresholds,
) -> Result<BenchDiffOutcome, BenchError> {
    let _run_span = tracing::info_span!(
        "bench_diff_from_history",
        history = %history_path.display(),
        current = %current_path.display(),
    )
    .entered();
    let current = report_store.load(current_path)?;
    let records = history_store.load_all(history_path)?;
    let baseline = latest_baseline(&records, &current.program)?;
    let diff = diff_reports(&baseline, &current, thresholds)?;
    let passed = diff.wall.verdict != DiffVerdict::Regressed
        && diff
            .instructions
            .as_ref()
            .is_none_or(|i| i.verdict != DiffVerdict::Regressed);
    tracing::debug!(program = %diff.program, passed, "history diff complete");
    Ok(BenchDiffOutcome {
        schema: crate::diff::BENCH_DIFF_SCHEMA.to_string(),
        baseline_path: format!("{}#latest", history_path.display()),
        current_path: current_path.display().to_string(),
        max_delta_pct: thresholds.max_delta_pct,
        diffs: vec![diff],
        passed,
    })
}
