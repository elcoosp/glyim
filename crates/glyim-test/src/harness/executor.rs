use super::collector::DiscoveredTest;
use super::compiler::{FrontendOnlyCompiler, PipelineCompiler, TestCompiler};
use super::strategy;
use crate::error::{FailureReason, TimeoutError};
use crate::harness::evidence::EvidenceBackend;
use glyim_diag::GlyimDiagnostic;
use glyim_span::FileId;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static NEXT_FILE_ID: AtomicU32 = AtomicU32::new(1);

fn next_file_id() -> FileId {
    FileId::from_raw(NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Clone, Debug)]
/// `TestOutcome`.
pub enum TestOutcome {
    /// `Variant`.
    Passed,
    /// `Variant`.
    Failed {
        /// reason field.
        reason: FailureReason,
    },
    /// `Variant`.
    Ignored,
}

#[derive(Clone, Debug)]
/// `TestResult`.
pub struct TestResult {
    /// `Struct`.
    pub test: Arc<DiscoveredTest>,
    /// `Struct`.
    pub revision: String,
    /// `Struct`.
    pub outcome: TestOutcome,
    /// `Struct`.
    pub duration: Duration,
    /// `Struct`.
    pub diagnostics: Vec<GlyimDiagnostic>,
}

/// `TestExecutor`.
pub struct TestExecutor {
    bless: bool,
    /// TEST-3: unmet fixture needs become hard failures instead of
    /// silent `Ignored`. Set by `TestPlan.no_skip`.
    no_skip: bool,
    max_concurrent: usize,
    target_triple: String,
    compiler: Arc<dyn TestCompiler>,
    /// The backend the chosen compiler actually drives — the evidence
    /// record's `backend` value (M1.5). `FrontendOnlyCompiler` never runs
    /// a program, so it records the GAM oracle tag.
    backend: EvidenceBackend,
}

impl TestExecutor {
    /// new.
    #[must_use]
    pub fn new(bless: bool, no_skip: bool, max_concurrent: usize, use_pipeline: bool) -> Self {
        let compiler: Arc<dyn TestCompiler>;
        let backend;
        if use_pipeline {
            // The `runtime` integration test enables `real_llvm` via the
            // `GLYIM_TEST_REAL_LLVM` env var (set only on the Linux CI runner),
            // which swaps in the real `LlvmBackend` so run-pass fixtures are
            // linked and executed for real. Other hosts keep the hermetic
            // `MockCodegen` path.
            let real_llvm = std::env::var("GLYIM_TEST_REAL_LLVM").is_ok();
            backend = if real_llvm {
                EvidenceBackend::Llvm
            } else {
                EvidenceBackend::Gam
            };
            compiler = Arc::new(
                PipelineCompiler::new(Arc::new(crate::mock::MockCodegen::new()))
                    .with_real_llvm_if(real_llvm),
            );
        } else {
            backend = EvidenceBackend::Gam;
            compiler = Arc::new(FrontendOnlyCompiler);
        }

        Self {
            bless,
            no_skip,
            max_concurrent,
            // Default to the *host* triple, not a hardcoded Linux one. Fixtures
            // carry `// only-target: <triple>` to declare the triple their
            // produced artifact executes on (the LLVM backend can *cross*-
            // compile an ELF for `x86_64-unknown-linux-gnu` from any host, but
            // only a Linux host can *run* that ELF without a user-mode
            // emulator). Defaulting to Linux made the runner try to execute
            // Linux ELFs on macOS, where every such fixture correctly
            // reported `FAILED` rather than `Ignored` — polluting the
            // summary with false regressions. With the host default, a
            // Linux-only fixture is `Ignored` on macOS and `RunPass` on
            // Linux, which is what the fixtures actually intend.
            target_triple: host_target_triple(),
            compiler,
            backend,
        }
    }

    /// The backend evidence records should name for fixtures run by this
    /// executor (M1.5).
    #[must_use]
    pub fn backend(&self) -> EvidenceBackend {
        self.backend
    }

    /// `with_target_triple`.
    #[must_use]
    pub fn with_target_triple(mut self, triple: impl Into<String>) -> Self {
        self.target_triple = triple.into();
        self
    }

    /// `with_compiler`.
    #[must_use]
    pub fn with_compiler(mut self, compiler: Arc<dyn TestCompiler>) -> Self {
        self.compiler = compiler;
        self
    }

    /// `run_sequential`.
    #[must_use]
    pub fn run_sequential(&self, tests: &[Arc<DiscoveredTest>]) -> Vec<TestResult> {
        tests
            .iter()
            .flat_map(|t| {
                let revs: Vec<String> = t.revisions.clone();
                revs.into_iter().map(|r| {
                    execute_test(
                        Arc::clone(t),
                        r,
                        Arc::clone(&self.compiler),
                        self.bless,
                        self.no_skip,
                        self.target_triple.clone(),
                    )
                })
            })
            .collect()
    }

    /// `run_parallel`.
    #[must_use]
    pub fn run_parallel(&self, tests: &[Arc<DiscoveredTest>]) -> Vec<TestResult> {
        // Micro-debt fix (M1.6 pass): the previous `build().unwrap()` turned
        // an OS thread-creation failure (RLIMIT_NPROC, cgroup caps, a
        // max_concurrent larger than the kernel allows) into a panic that
        // killed the whole test process — losing every result instead of
        // degrading gracefully. Falling back to the sequential path keeps
        // the run correct (just slower) and the reason visible in tracing.
        let pool = match rayon::ThreadPoolBuilder::new()
            .num_threads(self.max_concurrent)
            .build()
        {
            Ok(pool) => Some(pool),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    max_concurrent = self.max_concurrent,
                    "rayon thread-pool build failed; running the corpus sequentially instead"
                );
                None
            }
        };
        let Some(pool) = pool else {
            return self.run_sequential(tests);
        };
        let tests_c: Vec<Arc<DiscoveredTest>> = tests.to_vec();
        let compiler = Arc::clone(&self.compiler);
        let bless = self.bless;
        let no_skip = self.no_skip;
        let target_triple = self.target_triple.clone();

        // Progress reporting (plan §24.1): as each parallel task finishes, send
        // its (name, outcome) over a channel to a reporting thread that prints
        // cumulative progress ("X/Y passed=P failed=F ignored=I"). The channel is
        // bounded-ish by backpressure: the reporting thread drains promptly, so a
        // slow consumer can't grow memory without bound in practice.
        let total: usize = tests_c.iter().map(|t| t.revisions.len()).sum();
        let (progress_tx, progress_rx) = std::sync::mpsc::channel::<(String, TestOutcome)>();
        let progress_tx = std::sync::Arc::new(progress_tx);
        let tx_for_tasks = std::sync::Arc::clone(&progress_tx);
        let reporter = std::thread::spawn(move || {
            let mut done = 0usize;
            let mut passed = 0usize;
            let mut failed = 0usize;
            let mut ignored = 0usize;
            while let Ok((name, outcome)) = progress_rx.recv() {
                done += 1;
                match outcome {
                    TestOutcome::Passed => passed += 1,
                    TestOutcome::Failed { .. } => failed += 1,
                    TestOutcome::Ignored => ignored += 1,
                }
                eprintln!(
                    "[{done}/{total}] passed={passed} failed={failed} ignored={ignored} :: {name}"
                );
                if done >= total {
                    break;
                }
            }
        });

        let results = pool.install(move || {
            tests_c
                .par_iter()
                .flat_map(|t| {
                    let revs: Vec<String> = t.revisions.clone();
                    let comp = Arc::clone(&compiler);
                    let tgt = target_triple.clone();
                    let tx = Arc::clone(&tx_for_tasks);
                    revs.into_iter()
                        .map(move |r| {
                            let res = execute_test(
                                Arc::clone(t),
                                r,
                                Arc::clone(&comp),
                                bless,
                                no_skip,
                                tgt.clone(),
                            );
                            let _ = tx.send((res.test.name.clone(), res.outcome.clone()));
                            res
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        });
        drop(progress_tx);
        let _ = reporter.join();
        results
    }
}

fn execute_test(
    test: Arc<DiscoveredTest>,
    revision: String,
    compiler: Arc<dyn TestCompiler>,
    bless: bool,
    no_skip: bool,
    target_triple: String,
) -> TestResult {
    let span = tracing::info_span!("test", name = %test.name, revision = %revision);
    let _enter = span.enter();
    let start = std::time::Instant::now();

    if test.config.ignore {
        return TestResult {
            test,
            revision,
            outcome: TestOutcome::Ignored,
            duration: start.elapsed(),
            diagnostics: Vec::new(),
        };
    }

    // `needs-llvm` is a *capability* annotation: the fixture only passes when
    // the real `LlvmBackend` compiled, linked and executed it (118a18d1: the
    // MIR interpreter panics on `ref`-pattern projections). The signal for
    // that is `GLYIM_TEST_REAL_LLVM` — the same env var that makes
    // `PipelineCompiler` swap in the real backend and produce a runnable
    // executable. Un-skipping on mere toolchain presence (`GLYIM_LLVM`, the
    // pre-dependency-update key) ran these fixtures under the interpreter,
    // where they can only fail.
    if test.config.needs_llvm && std::env::var("GLYIM_TEST_REAL_LLVM").is_err() {
        let outcome = if no_skip {
            TestOutcome::Failed {
                reason: crate::error::FailureReason::NeedUnsatisfiable {
                    need: "needs-llvm".to_string(),
                    detail: "GLYIM_TEST_REAL_LLVM is not set (the real LLVM backend would not run this fixture)".to_string(),
                },
            }
        } else {
            TestOutcome::Ignored
        };
        return TestResult {
            test,
            revision,
            outcome,
            duration: start.elapsed(),
            diagnostics: Vec::new(),
        };
    }

    if let Some(ref target) = test.config.only_target
        && target != &target_triple
    {
        let outcome = if no_skip {
            TestOutcome::Failed {
                reason: crate::error::FailureReason::NeedUnsatisfiable {
                    need: "only-target".to_string(),
                    detail: format!("fixture requires {target}, host is {target_triple}"),
                },
            }
        } else {
            TestOutcome::Ignored
        };
        return TestResult {
            test,
            revision,
            outcome,
            duration: start.elapsed(),
            diagnostics: Vec::new(),
        };
    }

    let timeout = Duration::from_secs(test.config.timeout_secs);
    let test_clone = Arc::clone(&test);
    let revision_clone = revision.clone();

    let result = run_with_timeout(timeout, move || {
        execute_inner(&test_clone, &revision_clone, &*compiler, bless)
    });

    let (outcome, diagnostics) = match result {
        Ok((outcome, diags)) => (outcome, diags),
        Err(TimeoutError { timeout_secs }) => (
            TestOutcome::Failed {
                reason: FailureReason::TimeoutExceeded { timeout_secs },
            },
            Vec::new(),
        ),
    };

    let duration = start.elapsed();
    tracing::info!(duration_ms = duration.as_millis(), "done");

    // M1.6 strict-xfail witness inversion (plan §3.3): a compile-fail
    // fixture carrying `//@ known-gap: GAP-NNNN` is the reproducible
    // witness of an open readiness gap. While the gap is open the compiler
    // must *reject* the witness — the compile-fail expectations hold and
    // the outcome passes through unchanged. If the compiler instead
    // *accepts* the witness (no error diagnostics at all), the generic
    // `DiagnosticMismatch` would read as "test bug"; it actually means the
    // gap has CLOSED, and the strict-xfail contract demands a loud,
    // self-describing failure that tells the author how to retire the
    // witness.
    let outcome = apply_witness_inversion(
        test.config.known_gap.as_deref(),
        test.config.mode,
        outcome,
        &diagnostics,
    );

    TestResult {
        test,
        revision,
        outcome,
        duration,
        diagnostics,
    }
}

/// Apply the M1.6 strict-xfail inversion to a compile-fail outcome when the
/// fixture is a `//@ known-gap:` witness. Passing the outcome through a named
/// function (instead of inline branching) keeps the executor's happy path
/// flat and gives the seam one tracing span.
fn apply_witness_inversion(
    known_gap: Option<&str>,
    mode: super::config::TestMode,
    outcome: TestOutcome,
    diagnostics: &[GlyimDiagnostic],
) -> TestOutcome {
    let (Some(gap), super::config::TestMode::CompileFail) = (known_gap, mode) else {
        return outcome;
    };
    let witness_span = tracing::info_span!("witness", gap = %gap);
    let _enter = witness_span.enter();

    let compiler_accepted = diagnostics.iter().all(|d| !d.is_error());
    if compiler_accepted {
        tracing::warn!(
            gap = %gap,
            "known-gap witness closed: the compiler accepted a fixture it must reject"
        );
        return TestOutcome::Failed {
            reason: FailureReason::KnownGapClosed {
                gap: gap.to_string(),
            },
        };
    }
    outcome
}

fn execute_inner(
    test: &Arc<DiscoveredTest>,
    revision: &str,
    compiler: &dyn TestCompiler,
    bless: bool,
) -> (TestOutcome, Vec<GlyimDiagnostic>) {
    let mut flags = test.config.compile_flags.clone();
    if let Some(rev_flags) = test.config.revision_compile_flags.get(revision) {
        flags.extend(rev_flags.iter().cloned());
    }

    let file_id = next_file_id();

    let compile_span = tracing::info_span!("compile", file_id = file_id.to_raw());
    let output =
        compile_span.in_scope(|| compiler.compile(&test.source, &test.path, file_id, &flags));

    let run_timeout = std::time::Duration::from_secs(test.config.timeout_secs);

    let outcome = match test.config.mode {
        super::config::TestMode::CompilePass => {
            strategy::CompilePassStrategy.evaluate(&output.diagnostics, &test.source)
        }
        super::config::TestMode::CompileFail => strategy::CompileFailStrategy.evaluate(
            &output.diagnostics,
            &test.source,
            &test.config.error_patterns,
        ),
        super::config::TestMode::Ui => {
            strategy::UiTestStrategy.evaluate(&output, &test.source, &test.path, bless)
        }
        super::config::TestMode::RunPass => strategy::RunPassStrategy.evaluate(
            &output,
            &test.source,
            output.executable_path.as_deref(),
            &test.config,
            run_timeout,
        ),
        super::config::TestMode::RunFail => strategy::RunFailStrategy.evaluate(
            &output,
            &test.source,
            output.executable_path.as_deref(),
            &test.config,
            run_timeout,
        ),
    };

    (outcome, output.diagnostics)
}

fn run_with_timeout<F, R>(timeout: Duration, f: F) -> Result<R, TimeoutError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let timeout_secs = timeout.as_secs();

    std::thread::spawn(move || {
        let result = f();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => Ok(result),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(TimeoutError { timeout_secs }),
        // T144-PATCHED [HARNESS-4]: `Disconnected` means the worker thread
        // panicked (or dropped the sender) before sending a result. The
        // previous `Err(_)` collapsed this into a `TimeoutError`, hiding
        // the real ICE. Surface it distinctly so the caller can classify
        // the failure.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(TimeoutError { timeout_secs: 0 })
        }
    }
}

/// Best-effort Rust target triple for the host executing the tests. Mirrors
/// the mapping `glyim-cli::host_target_triple` uses for proc-macro builds so
/// the two agree on what "the host" means.
fn host_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (arch, os) {
        ("x86_64", "macos") => "x86_64-apple-darwin".to_string(),
        ("aarch64", "macos") => "aarch64-apple-darwin".to_string(),
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu".to_string(),
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu".to_string(),
        ("x86_64", "windows") => "x86_64-pc-windows-msvc".to_string(),
        ("aarch64", "windows") => "aarch64-pc-windows-msvc".to_string(),
        _ => format!("{arch}-{os}"),
    }
}
