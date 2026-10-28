use super::collector::TestCollector;
use super::config::TestMode;
use super::executor::TestExecutor;
use super::reporter::TestReporter;
use crate::error::TestDiscoveryError;
use std::path::PathBuf;
use std::time::Duration;

/// TestRunner.
pub struct TestRunner {
    root: PathBuf,
    mode_override: Option<TestMode>,
    parallel: bool,
    filter: Option<String>,
    timeout: Duration,
    max_concurrent: usize,
    use_pipeline: bool,
}

impl TestRunner {
    /// new.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            mode_override: None,
            parallel: true,
            filter: None,
            timeout: Duration::from_secs(60),
            max_concurrent: 8,
            use_pipeline: true,
        }
    }
    /// mode.
    pub fn mode(mut self, mode: TestMode) -> Self {
        self.mode_override = Some(mode);
        self
    }
    /// parallel.
    pub fn parallel(mut self, yes: bool) -> Self {
        self.parallel = yes;
        self
    }
    /// filter.
    pub fn filter(mut self, f: impl Into<String>) -> Self {
        self.filter = Some(f.into());
        self
    }
    /// timeout.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }
    /// max_concurrent.
    pub fn max_concurrent(mut self, n: usize) -> Self {
        self.max_concurrent = n;
        self
    }
    /// frontend_only.
    pub fn frontend_only(mut self) -> Self {
        self.use_pipeline = false;
        self
    }

    /// build.
    pub fn build(self) -> Result<TestPlan, TestDiscoveryError> {
        let collector = TestCollector::new(&self.root);
        let tests = collector.collect(self.filter.as_deref(), self.mode_override)?;
        Ok(TestPlan {
            tests,
            parallel: self.parallel,
            default_timeout: self.timeout,
            max_concurrent: self.max_concurrent,
            use_pipeline: self.use_pipeline,
            bless: resolve_bless_flag(
                std::env::var("CI").ok().as_deref(),
                std::env::var("GLYIM_BLESS").ok().as_deref(),
            ),
            verbose: std::env::var("GLYIM_TEST_SHOW_OUTPUT").is_ok(),
        })
    }
}

/// Resolve the `GLYIM_BLESS` flag with a CI safety gate (TEST-6).
///
/// `GLYIM_BLESS=1` tells the UI-test harness to *overwrite* the
/// `.expected` snapshot files instead of comparing against them, and to
/// report `Passed` for every fixture it rewrote (`strategy.rs`
/// `UiTestStrategy::evaluate`). That is the right behavior for a local
/// re-bless after an intentional diagnostic change, and a **hazard in
/// CI**: a stale or accidental `GLYIM_BLESS` in the CI environment would
/// turn every snapshot mismatch into a silent rewrite, so the job would
/// pass while discarding regressions.
///
/// This helper centralises the decision so it is testable without
/// touching the process environment. Contract:
///
///   * `CI` unset, `GLYIM_BLESS` unset  -> `false` (compare mode)
///   * `CI` unset, `GLYIM_BLESS` set    -> `true`  (local bless)
///   * `CI` set,   `GLYIM_BLESS` unset  -> `false` (compare mode)
///   * `CI` set,   `GLYIM_BLESS` set    -> **panic** — refuse to bless
///                                          in CI; the caller must
///                                          unset one or the other
///
/// The panic (rather than a silent `false`) is deliberate: a
/// misconfigured CI job should fail loudly with an actionable message,
/// not quietly stop blessing and leave the user wondering why their
/// snapshots didn't update. `CI`'s presence is checked with `.is_some()`
/// so any non-empty value counts, matching how GitHub Actions / GitLab /
/// Circle set it.
fn resolve_bless_flag(ci: Option<&str>, bless: Option<&str>) -> bool {
    let bless_requested = bless.is_some();
    let in_ci = ci.is_some();
    if bless_requested && in_ci {
        panic!(
            "GLYIM_BLESS is set but CI is also set: refusing to bless              snapshots in CI. `GLYIM_BLESS=1` rewrites `.expected` files              and reports every fixture as passing, which would silently              hide regressions. Unset GLYIM_BLESS for the CI run (or, for              a deliberate re-bless, run locally with CI unset)."
        );
    }
    bless_requested
}

/// TestPlan.
pub struct TestPlan {
    /// Struct.
    pub tests: Vec<std::sync::Arc<super::collector::DiscoveredTest>>,
    /// Struct.
    pub parallel: bool,
    /// Struct.
    pub default_timeout: Duration,
    /// Struct.
    pub max_concurrent: usize,
    #[allow(missing_docs)]
    pub use_pipeline: bool,
    /// Struct.
    pub bless: bool,
    /// Struct.
    pub verbose: bool,
}

/// ExecutionResult.
pub struct ExecutionResult {
    /// Struct.
    pub results: Vec<super::executor::TestResult>,
    /// Struct.
    pub summary: TestSummary,
}

impl TestPlan {
    /// execute.
    pub fn execute(self) -> ExecutionResult {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .try_init();

        if self.tests.is_empty() {
            eprintln!("no test files found");
            return ExecutionResult {
                results: Vec::new(),
                summary: TestSummary::default(),
            };
        }

        eprintln!("running {} tests", self.tests.len());

        let executor = TestExecutor::new(
            self.default_timeout,
            self.bless,
            self.verbose,
            self.max_concurrent,
            self.use_pipeline,
        );
        let results = if self.parallel {
            executor.run_parallel(&self.tests)
        } else {
            executor.run_sequential(&self.tests)
        };

        let reporter = TestReporter::new(self.verbose);
        let summary = reporter.report(&results);

        ExecutionResult { results, summary }
    }

    /// run.
    pub fn run(self) {
        let result = self.execute();
        if result.summary.failed > 0 {
            let failed_names: Vec<_> = result
                .results
                .iter()
                .filter(|r| matches!(r.outcome, super::executor::TestOutcome::Failed { .. }))
                .map(|r| format!("{} [{}]", r.test.name, r.revision))
                .collect();
            panic!(
                "{} tests FAILED: {}\n\
                 Run with GLYIM_TEST_SHOW_OUTPUT=1 for details.",
                result.summary.failed,
                failed_names.join(", ")
            );
        }
    }
}

#[derive(Clone, Debug, Default)]
/// TestSummary.
pub struct TestSummary {
    /// Struct.
    pub total: usize,
    /// Struct.
    pub passed: usize,
    /// Struct.
    pub failed: usize,
    /// Struct.
    pub ignored: usize,
}


#[cfg(test)]
mod resolve_bless_flag_tests {
    use super::resolve_bless_flag;

    #[test]
    fn neither_set_is_false() {
        assert!(!resolve_bless_flag(None, None));
    }

    #[test]
    fn bless_only_is_true() {
        assert!(resolve_bless_flag(None, Some("1")));
    }

    #[test]
    fn ci_only_is_false() {
        assert!(!resolve_bless_flag(Some("true"), None));
    }

    #[test]
    fn empty_strings_still_count_as_set() {
        // Both env vars are checked with `.is_some()`, not `.is_ok_and(!="")`:
        // an empty value still means "the operator asked for it".
        assert!(!resolve_bless_flag(Some(""), None));
        assert!(resolve_bless_flag(None, Some("")));
    }

    #[test]
    #[should_panic(expected = "refusing to bless")]
    fn ci_and_bless_together_panics() {
        // The gate itself — must fail loudly so a misconfigured CI job is
        // diagnosed at the point of misconfiguration, not as an
        // unexplained "snapshots didn't update".
        let _ = resolve_bless_flag(Some("true"), Some("1"));
    }
}
