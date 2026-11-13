//! Typed errors for every measurement seam.
//!
//! Every variant implements [`miette::Diagnostic`] with an actionable `help`
//! line — the same discipline as the `glyim-test` evidence seam (`EvidenceError`)
//! and the fixture-header parser (`TestConfigError`): a failure must name the
//! remedy, not just the symptom.

use std::fmt;

use crate::source::MeasurementSource;

/// Errors produced by the harness runner, parsers, and CLI.
#[derive(Debug, Clone, PartialEq)]
pub enum BenchError {
    /// The run plan itself is invalid (e.g. zero iterations).
    InvalidPlan {
        /// What is wrong with the plan.
        detail: String,
    },
    /// The requested counter source's wrapper program could not be launched
    /// or reported failure on its own `--version` probe.
    MeasurementSourceUnavailable {
        /// The source that was requested.
        source: MeasurementSource,
        /// The wrapper program that failed to launch (`perf`, `valgrind`).
        program: String,
    },
    /// `perf stat -x,` output could not be parsed into instruction/cycle
    /// counters.
    PerfStatParse {
        /// What went wrong (unexpected row shape, no `instructions` event).
        detail: String,
    },
    /// A callgrind output file had no parsable `summary:` line.
    CallgrindParse {
        /// What went wrong.
        detail: String,
    },
    /// The measured (or wrapper) program could not be spawned.
    CommandLaunch {
        /// The program that failed to spawn.
        program: String,
        /// The underlying IO error message.
        detail: String,
    },
    /// An iteration of the measured command exited non-zero (or was
    /// terminated by a signal).
    ChildFailed {
        /// The measured program.
        program: String,
        /// Exit code, or `None` when terminated by a signal.
        code: Option<i32>,
    },
    /// A report file could not be read.
    ReportRead {
        /// The report path.
        path: String,
        /// The underlying IO error message.
        detail: String,
    },
    /// A report file's contents are not a valid `glyim.bench/1` document.
    ReportParse {
        /// The deserialisation error message.
        detail: String,
    },
    /// A report file could not be written.
    ReportWrite {
        /// The report path.
        path: String,
        /// The underlying IO error message.
        detail: String,
    },
    /// The stability gate failed: the measured metric's coefficient of
    /// variation exceeded the budget.
    GateFailed {
        /// Which metric failed (`instructions` or `wall_ns`).
        metric: String,
        /// Measured `CoV`, in percent.
        cov_pct: f64,
        /// Allowed `CoV`, in percent.
        max_cov_pct: f64,
    },
    /// A corpus plan is invalid (empty families/seeds, or a `size` outside
    /// `1..=MAX_SIZE`).
    CorpusPlan {
        /// What is wrong with the plan.
        detail: String,
    },
    /// A corpus manifest file's contents are not a valid `glyim.corpus/1`
    /// document.
    CorpusManifestParse {
        /// The deserialisation error message.
        detail: String,
    },
    /// A committed manifest row no longer regenerates byte-identically: a
    /// generator template drifted after the manifest was committed.
    CorpusIdentityMismatch {
        /// The drifted row's family.
        family: String,
        /// The drifted row's seed.
        seed: u64,
        /// The identity recorded in the manifest.
        expected: u64,
        /// The identity of the regenerated program.
        actual: u64,
    },
    /// A budgets file could not be read (or written).
    BudgetsRead {
        /// The budgets file path.
        path: String,
        /// The underlying IO error message.
        detail: String,
    },
    /// A budgets file's contents are not a valid `glyim.bench-budgets/1`
    /// document (bad TOML, wrong schema tag, duplicate programs, or a row
    /// without any bound).
    BudgetsParse {
        /// What violates the budgets contract.
        detail: String,
    },
    /// A history file could not be read (or appended to).
    HistoryRead {
        /// The history file path.
        path: String,
        /// The underlying IO error message.
        detail: String,
    },
    /// A history line is not a valid `glyim.bench-history/1` record.
    HistoryParse {
        /// What violates the history contract.
        detail: String,
    },
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan { detail } => write!(f, "invalid bench plan: {detail}"),
            Self::MeasurementSourceUnavailable { source, program } => write!(
                f,
                "measurement source {source} is unavailable: `{program}` could not be launched"
            ),
            Self::PerfStatParse { detail } => {
                write!(f, "cannot parse `perf stat` output: {detail}")
            }
            Self::CallgrindParse { detail } => {
                write!(f, "cannot parse callgrind output: {detail}")
            }
            Self::CommandLaunch { program, detail } => {
                write!(f, "cannot launch `{program}`: {detail}")
            }
            Self::ChildFailed { program, code } => match code {
                Some(code) => write!(f, "`{program}` exited with status {code}"),
                None => write!(f, "`{program}` was terminated by a signal"),
            },
            Self::ReportRead { path, detail } => write!(f, "cannot read {path}: {detail}"),
            Self::ReportParse { detail } => write!(f, "invalid bench report: {detail}"),
            Self::ReportWrite { path, detail } => write!(f, "cannot write {path}: {detail}"),
            Self::GateFailed {
                metric,
                cov_pct,
                max_cov_pct,
            } => write!(
                f,
                "stability gate failed: {metric} CoV {cov_pct:.3}% exceeds budget {max_cov_pct:.3}%"
            ),
            Self::CorpusPlan { detail } => write!(f, "invalid corpus plan: {detail}"),
            Self::CorpusManifestParse { detail } => {
                write!(f, "invalid corpus manifest: {detail}")
            }
            Self::CorpusIdentityMismatch {
                family,
                seed,
                expected,
                actual,
            } => write!(
                f,
                "corpus program {family}/{seed} drifted: manifest identity {expected:016x}, regenerated {actual:016x}"
            ),
            Self::BudgetsRead { path, detail } => {
                write!(f, "cannot read budgets file {path}: {detail}")
            }
            Self::BudgetsParse { detail } => write!(f, "invalid budgets file: {detail}"),
            Self::HistoryRead { path, detail } => {
                write!(f, "cannot read history file {path}: {detail}")
            }
            Self::HistoryParse { detail } => write!(f, "invalid history record: {detail}"),
        }
    }
}

impl std::error::Error for BenchError {}

impl miette::Diagnostic for BenchError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(match self {
            Self::InvalidPlan { .. } => "glyim-bench::invalid_plan",
            Self::MeasurementSourceUnavailable { .. } => "glyim-bench::source_unavailable",
            Self::PerfStatParse { .. } => "glyim-bench::perf_stat_parse",
            Self::CallgrindParse { .. } => "glyim-bench::callgrind_parse",
            Self::CommandLaunch { .. } => "glyim-bench::command_launch",
            Self::ChildFailed { .. } => "glyim-bench::child_failed",
            Self::ReportRead { .. } => "glyim-bench::report_read",
            Self::ReportParse { .. } => "glyim-bench::report_parse",
            Self::ReportWrite { .. } => "glyim-bench::report_write",
            Self::GateFailed { .. } => "glyim-bench::gate_failed",
            Self::CorpusPlan { .. } => "glyim-bench::corpus_plan",
            Self::CorpusManifestParse { .. } => "glyim-bench::corpus_manifest_parse",
            Self::CorpusIdentityMismatch { .. } => "glyim-bench::corpus_identity_mismatch",
            Self::BudgetsRead { .. } => "glyim-bench::budgets_read",
            Self::BudgetsParse { .. } => "glyim-bench::budgets_parse",
            Self::HistoryRead { .. } => "glyim-bench::history_read",
            Self::HistoryParse { .. } => "glyim-bench::history_parse",
        }))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(match self {
            Self::InvalidPlan { .. } => {
                "pass a positive `--iterations` count (the M2.2 gate expects 10)".to_string()
            }
            Self::MeasurementSourceUnavailable { source, .. } => match source {
                MeasurementSource::PerfStat => {
                    "install `perf` (linux-tools for your kernel) or run with \
                     `--source wall` for wall-clock-only sampling"
                        .to_string()
                }
                MeasurementSource::Callgrind => {
                    "install `valgrind` (the callgrind tool ships with it) or run \
                     with `--source perf` / `--source wall`"
                        .to_string()
                }
                MeasurementSource::WallOnly => {
                    "wall-clock sampling needs no external tools; this indicates \
                     a broken PATH — check that the shell can spawn processes"
                        .to_string()
                }
            },
            Self::PerfStatParse { .. } => {
                "the harness expects `perf stat -x,` CSV rows (count,unit,event,..); \
                 re-run with `--source wall` to isolate a format drift, and pin \
                 the perf version in the environment"
                    .to_string()
            }
            Self::CallgrindParse { .. } => {
                "callgrind writes a final `summary: <Ir>` line when the run \
                 completes; ensure `--callgrind-out-file` points at the file \
                 this harness parses and that the child was not killed"
                    .to_string()
            }
            Self::CommandLaunch { program, .. } => {
                format!("check that `{program}` exists and is executable on PATH")
            }
            Self::ChildFailed { .. } => {
                "every measured iteration must succeed for the samples to be \
                 comparable; fix the child's failure (or its exit code) first"
                    .to_string()
            }
            Self::ReportRead { .. } => {
                "check the path passed to `glyim-bench gate` / `--json` and its \
                 permissions"
                    .to_string()
            }
            Self::ReportParse { .. } => "reports are `glyim.bench/1` JSON documents with a strict \
                 (deny_unknown_fields) schema; regenerate with \
                 `glyim-bench run --json <path> -- <command>`"
                .to_string(),
            Self::ReportWrite { .. } => {
                "check that the parent directory exists and is writable; the \
                 harness does not create nested directories implicitly"
                    .to_string()
            }
            Self::GateFailed { .. } => {
                "raise `--iterations` (the M2.2 gate expects 10 runs), pin the \
                 machine (disable turbo/ASLR where possible), or investigate \
                 the outlier samples in the report before trusting any delta"
                    .to_string()
            }
            Self::CorpusPlan { .. } => {
                "generate with `size` in 1..=256 (the MAX_SIZE bound that keeps \
                 every reference exit inside i32) and non-empty families/seeds; \
                 the M2.3 slice defaults to 24 elements per program"
                    .to_string()
            }
            Self::CorpusManifestParse { .. } => {
                "manifests are `glyim.corpus/1` JSON documents with a strict \
                 (deny_unknown_fields) schema; regenerate one with \
                 `cargo run -p glyim-bench --example corpus_walk`"
                    .to_string()
            }
            Self::CorpusIdentityMismatch { .. } => {
                "a generator template changed after the manifest was committed; \
                 either revert the template edit or regenerate and re-commit the \
                 manifest (the identity column is the M2.3 \"corpora committed \
                 with hashes\" contract)"
                    .to_string()
            }
            Self::BudgetsRead { .. } => {
                "check the path passed to `glyim-bench budget --budgets` and its \
                 permissions; seed a starter file with \
                 `cargo run -p glyim-bench --example budget_check`"
                    .to_string()
            }
            Self::BudgetsParse { .. } => {
                "budgets files are `glyim.bench-budgets/1` TOML documents with a \
                 strict (deny_unknown_fields) schema: unique `[[budget]]` rows, \
                 each naming a `program` and at least one of `max_wall_mean_ns` / \
                 `max_instructions_mean`"
                    .to_string()
            }
            Self::HistoryRead { .. } => {
                "check the path passed to `glyim-bench history --history`; the file \
                 is created by the first `glyim-bench history record` append"
                    .to_string()
            }
            Self::HistoryParse { .. } => {
                "history files are JSONL: one `glyim.bench-history/1` record per \
                 line, each wrapping a `glyim.bench/1` report; rebuild a file with \
                 `glyim-bench history record`"
                    .to_string()
            }
        }))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::BenchError;
    use crate::source::MeasurementSource;

    #[test]
    fn every_variant_renders_with_help_text() {
        let errors = [
            BenchError::InvalidPlan {
                detail: "zero iterations".into(),
            },
            BenchError::MeasurementSourceUnavailable {
                source: MeasurementSource::PerfStat,
                program: "perf".into(),
            },
            BenchError::PerfStatParse {
                detail: "no instructions row".into(),
            },
            BenchError::CallgrindParse {
                detail: "no summary line".into(),
            },
            BenchError::CommandLaunch {
                program: "nope".into(),
                detail: "No such file".into(),
            },
            BenchError::ChildFailed {
                program: "fail".into(),
                code: Some(3),
            },
            BenchError::ReportRead {
                path: "r.json".into(),
                detail: "missing".into(),
            },
            BenchError::ReportParse {
                detail: "bad json".into(),
            },
            BenchError::ReportWrite {
                path: "r.json".into(),
                detail: "denied".into(),
            },
            BenchError::GateFailed {
                metric: "instructions".into(),
                cov_pct: 0.5,
                max_cov_pct: 0.1,
            },
            BenchError::CorpusPlan {
                detail: "size 0".into(),
            },
            BenchError::CorpusManifestParse {
                detail: "bad json".into(),
            },
            BenchError::CorpusIdentityMismatch {
                family: "wide".into(),
                seed: 1,
                expected: 0xab,
                actual: 0xcd,
            },
            BenchError::BudgetsRead {
                path: "perf/budgets.toml".into(),
                detail: "missing".into(),
            },
            BenchError::BudgetsParse {
                detail: "duplicate program".into(),
            },
            BenchError::HistoryRead {
                path: "perf/history/fib.jsonl".into(),
                detail: "denied".into(),
            },
            BenchError::HistoryParse {
                detail: "bad line".into(),
            },
        ];
        for err in errors {
            assert!(!err.to_string().is_empty(), "{err:?} must render");
            let help = miette::Diagnostic::help(&err)
                .expect("every BenchError variant must carry help text");
            assert!(
                !help.to_string().is_empty(),
                "{err:?} help text must be non-empty"
            );
            assert_eq!(
                miette::Diagnostic::severity(&err),
                Some(miette::Severity::Error)
            );
        }
    }

    #[test]
    fn display_names_the_program_and_code() {
        let err = BenchError::ChildFailed {
            program: "glyim".into(),
            code: Some(101),
        };
        assert_eq!(err.to_string(), "`glyim` exited with status 101");
    }
}
