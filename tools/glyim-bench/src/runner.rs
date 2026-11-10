//! The iteration runner: N executions of one command, one
//! [`BenchSample`](crate::report::BenchSample) each.
//!
//! Counter sources wrap the child in the source's tool (`perf stat` /
//! `valgrind --tool=callgrind`); the tool's counts are attributed to the
//! child only, while the wall column necessarily includes the wrapper's
//! startup. Child RSS uses `getrusage(RUSAGE_CHILDREN)`, which reports the
//! running maximum over reaped children — monotone across iterations, and
//! exact for the maximum of the run.

use std::process::{Command, Stdio};
use std::time::Instant;

use tracing::info_span;

use crate::error::BenchError;
use crate::report::{BenchReport, BenchSample, GateOutcome};
use crate::source::{MeasurementSource, PerfCounters, parse_callgrind_summary, parse_perf_stat};
use crate::stats::{self, SampleStats};

/// A planned bench run: what to execute, how often, and with which counter
/// source.
#[derive(Clone, Debug, PartialEq)]
pub struct RunPlan {
    /// Program under measurement (never the wrapper).
    pub program: String,
    /// Arguments passed to `program`.
    pub args: Vec<String>,
    /// Iterations to execute.
    pub iterations: usize,
    /// Counter source.
    pub source: MeasurementSource,
    /// Allowed coefficient of variation for the stability gate, in percent.
    pub max_cov_pct: f64,
}

impl RunPlan {
    /// A plan for `program` with the M2.2 defaults: 10 iterations, wall-only
    /// source until a counter tool is requested or detected, the
    /// [`stats::DEFAULT_MAX_COV_PCT`] budget.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            iterations: 10,
            source: MeasurementSource::WallOnly,
            max_cov_pct: stats::DEFAULT_MAX_COV_PCT,
        }
    }

    /// Set the iteration count.
    #[must_use]
    pub const fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }

    /// Set the counter source.
    #[must_use]
    pub const fn with_source(mut self, source: MeasurementSource) -> Self {
        self.source = source;
        self
    }

    /// Set the stability-gate budget (percent).
    #[must_use]
    pub const fn with_max_cov_pct(mut self, max_cov_pct: f64) -> Self {
        self.max_cov_pct = max_cov_pct;
        self
    }

    /// Append one argument for the measured program.
    #[must_use]
    pub fn push_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }
}

/// Probe whether a wrapper program is launchable (`--version` exits 0).
fn probe_wrapper(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Pick a source automatically: `perf stat` when `perf` works, otherwise
/// wall-only (callgrind is always opt-in — it is the slowest source).
#[must_use]
pub fn detect_source() -> MeasurementSource {
    if probe_wrapper("perf") {
        MeasurementSource::PerfStat
    } else {
        tracing::info!(
            "perf not detected; falling back to wall-only sampling (RSS still collected on Linux)"
        );
        MeasurementSource::WallOnly
    }
}

/// Child peak RSS in KiB via `getrusage(RUSAGE_CHILDREN)` — the running
/// maximum over reaped children. Linux only (macOS reports bytes in the same
/// field, and the contract here is KiB; `None` elsewhere).
fn child_max_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `usage` is a valid, fully-initialised `rusage` buffer owned
        // by this call; `getrusage` only writes through the pointer.
        let rc = unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) };
        if rc == 0 {
            return Some(usage.ru_maxrss.max(0) as u64);
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    None
}

fn scratch_path(iteration: usize) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "glyim-bench-{}-{iteration}.out",
        std::process::id()
    ))
}

/// Execute one iteration and collect its counters.
fn run_once(plan: &RunPlan, iteration: usize) -> Result<BenchSample, BenchError> {
    let out_path = scratch_path(iteration);
    let out_str = out_path.display().to_string();

    let (mut command, needs_counters) = match plan.source {
        MeasurementSource::PerfStat => {
            let mut cmd = Command::new("perf");
            cmd.args([
                "stat",
                "-x,",
                "-e",
                "instructions,cycles",
                "-o",
                &out_str,
                "--",
            ]);
            (cmd, true)
        }
        MeasurementSource::Callgrind => {
            let mut cmd = Command::new("valgrind");
            cmd.arg("--tool=callgrind")
                .arg(format!("--callgrind-out-file={out_str}"));
            (cmd, true)
        }
        MeasurementSource::WallOnly => (Command::new(&plan.program), false),
    };
    if plan.source == MeasurementSource::WallOnly {
        command.args(&plan.args);
    } else {
        command.arg(&plan.program).args(&plan.args);
    }
    command.stdin(Stdio::null());

    let start = Instant::now();
    let status = command.status().map_err(|err| BenchError::CommandLaunch {
        program: plan
            .source
            .wrapper_program()
            .unwrap_or(plan.program.as_str())
            .to_string(),
        detail: err.to_string(),
    })?;
    let wall_ns = start.elapsed().as_nanos() as u64;

    if !status.success() {
        return Err(BenchError::ChildFailed {
            program: plan.program.clone(),
            code: status.code(),
        });
    }

    let counters = if needs_counters {
        let text = std::fs::read_to_string(&out_path).map_err(|err| BenchError::ReportRead {
            path: out_str.clone(),
            detail: err.to_string(),
        })?;
        let parsed = match plan.source {
            MeasurementSource::PerfStat => parse_perf_stat(&text)?,
            MeasurementSource::Callgrind => {
                let instructions = parse_callgrind_summary(&text)?;
                PerfCounters {
                    instructions: Some(instructions),
                    cycles: None,
                }
            }
            MeasurementSource::WallOnly => PerfCounters::default(),
        };
        if let Err(err) = std::fs::remove_file(&out_path) {
            tracing::debug!(path = %out_str, error = %err, "scratch counter file cleanup failed");
        }
        parsed
    } else {
        PerfCounters::default()
    };

    tracing::debug!(
        iteration,
        wall_ns,
        instructions = ?counters.instructions,
        "sample collected"
    );

    Ok(BenchSample {
        wall_ns,
        instructions: counters.instructions,
        cycles: counters.cycles,
        rss_kb: child_max_rss_kb(),
        alloc_count: None,
        alloc_bytes: None,
    })
}

/// Execute the plan and assemble the report.
///
/// # Errors
///
/// [`BenchError::InvalidPlan`] for zero iterations, and any
/// launch/parse/failure error from the iterations themselves.
pub fn run_iterated(plan: &RunPlan) -> Result<BenchReport, BenchError> {
    if plan.iterations == 0 {
        return Err(BenchError::InvalidPlan {
            detail: "at least one iteration is required".to_string(),
        });
    }
    let _run_span = info_span!(
        "bench_run",
        program = %plan.program,
        iterations = plan.iterations,
        source = plan.source.as_str(),
    )
    .entered();

    if let Some(wrapper) = plan.source.wrapper_program()
        && !probe_wrapper(wrapper)
    {
        return Err(BenchError::MeasurementSourceUnavailable {
            source: plan.source,
            program: wrapper.to_string(),
        });
    }

    let mut samples = Vec::with_capacity(plan.iterations);
    for iteration in 0..plan.iterations {
        samples.push(run_once(plan, iteration)?);
    }

    let walls: Vec<u64> = samples.iter().map(|s| s.wall_ns).collect();
    let wall_stats: SampleStats = stats::summarize(&walls).expect("iterations >= 1");
    let ins_values: Vec<u64> = samples.iter().filter_map(|s| s.instructions).collect();
    let ins_stats = stats::summarize(&ins_values);

    // Borrow through `as_ref` instead of `is_some` + `expect`: `expect`
    // takes `self` by value, so the old shape moved `ins_stats` out from
    // under its later use in the report (a hard E0382 under rustc 1.99's
    // mover). Same semantics — instructions when measured, wall otherwise.
    let (metric, cov_pct) = match ins_stats.as_ref() {
        Some(ins) => ("instructions", ins.cov_pct),
        None => ("wall_ns", wall_stats.cov_pct),
    };
    let gate = GateOutcome {
        metric: metric.to_string(),
        cov_pct,
        max_cov_pct: plan.max_cov_pct,
        passed: stats::gate_passes(cov_pct, plan.max_cov_pct),
    };

    Ok(BenchReport::new(
        plan.program.clone(),
        plan.args.clone(),
        plan.source,
        plan.iterations,
        samples,
        ins_stats,
        wall_stats,
        gate,
    ))
}

#[cfg(test)]
mod tests {
    use super::{RunPlan, run_iterated};
    use crate::error::BenchError;
    use crate::source::MeasurementSource;

    #[test]
    #[cfg(unix)]
    fn wall_only_plan_collects_one_sample_per_iteration() {
        let plan = RunPlan::new("sh")
            .with_iterations(3)
            .with_source(MeasurementSource::WallOnly)
            .with_max_cov_pct(f64::INFINITY)
            .push_arg("-c")
            .push_arg("exit 0");
        let report = run_iterated(&plan).expect("runs");
        assert_eq!(report.samples.len(), 3);
        assert_eq!(report.iterations, 3);
        assert!(report.instructions.is_none());
        assert_eq!(report.gate.metric, "wall_ns");
        assert!(report.gate.passed, "infinite budget always passes");
        assert_eq!(report.program, "sh");
        assert_eq!(report.args, vec!["-c".to_string(), "exit 0".to_string()]);
    }

    #[test]
    #[cfg(unix)]
    fn failing_child_is_a_hard_error() {
        let plan = RunPlan::new("sh")
            .with_iterations(2)
            .with_source(MeasurementSource::WallOnly)
            .push_arg("-c")
            .push_arg("exit 3");
        let err = run_iterated(&plan).expect_err("child fails");
        assert!(matches!(err, BenchError::ChildFailed { code: Some(3), .. }));
    }

    #[test]
    fn zero_iterations_is_an_invalid_plan() {
        let err = run_iterated(&RunPlan::new("sh").with_iterations(0)).expect_err("invalid");
        assert!(matches!(err, BenchError::InvalidPlan { .. }));
    }

    #[test]
    #[cfg(unix)]
    fn unavailable_wrapper_is_reported_with_help() {
        // A source whose wrapper is a name that cannot exist on PATH.
        let plan = RunPlan::new("sh")
            .with_source(MeasurementSource::PerfStat)
            .with_iterations(1);
        // Only assert when `perf` genuinely is absent, so the test stays
        // truthful on machines that have it.
        if std::process::Command::new("perf")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_err()
        {
            let err = run_iterated(&plan).expect_err("no perf");
            assert!(matches!(
                err,
                BenchError::MeasurementSourceUnavailable { .. }
            ));
            let help = miette::Diagnostic::help(&err).expect("help");
            assert!(help.to_string().contains("--source wall"));
        }
    }
}
