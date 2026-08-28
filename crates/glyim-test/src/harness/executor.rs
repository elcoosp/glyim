use super::collector::DiscoveredTest;
use super::compiler::{FrontendOnlyCompiler, PipelineCompiler, TestCompiler};
use super::strategy;
use crate::error::{FailureReason, TimeoutError};
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
/// TestOutcome.
pub enum TestOutcome {
/// Variant.
    Passed,
/// Variant.
    Failed {
        /// reason field.
        reason: FailureReason,
    },
/// Variant.
    Ignored,
}

#[derive(Clone, Debug)]
/// TestResult.
pub struct TestResult {
/// Struct.
    pub test: Arc<DiscoveredTest>,
/// Struct.
    pub revision: String,
/// Struct.
    pub outcome: TestOutcome,
/// Struct.
    pub duration: Duration,
/// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
}

#[allow(dead_code)]
/// TestExecutor.
pub struct TestExecutor {
    default_timeout: Duration,
    bless: bool,
    verbose: bool,
    max_concurrent: usize,
    target_triple: String,
    compiler: Arc<dyn TestCompiler>,
}

impl TestExecutor {
/// new.
    pub fn new(
        default_timeout: Duration,
        bless: bool,
        verbose: bool,
        max_concurrent: usize,
        use_pipeline: bool,
    ) -> Self {
        let compiler: Arc<dyn TestCompiler> = if use_pipeline {
            // The `runtime` integration test enables `real_llvm` via the
            // `GLYIM_TEST_REAL_LLVM` env var (set only on the Linux CI runner),
            // which swaps in the real `LlvmBackend` so run-pass fixtures are
            // linked and executed for real. Other hosts keep the hermetic
            // `MockCodegen` path.
            let real_llvm = std::env::var("GLYIM_TEST_REAL_LLVM").is_ok();
            Arc::new(PipelineCompiler::new(Arc::new(
                crate::mock::MockCodegen::new(),
            )).with_real_llvm_if(real_llvm))
        } else {
            Arc::new(FrontendOnlyCompiler)
        };

        Self {
            default_timeout,
            bless,
            verbose,
            max_concurrent,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            compiler,
        }
    }

/// with_target_triple.
    pub fn with_target_triple(mut self, triple: impl Into<String>) -> Self {
        self.target_triple = triple.into();
        self
    }

/// with_compiler.
    pub fn with_compiler(mut self, compiler: Arc<dyn TestCompiler>) -> Self {
        self.compiler = compiler;
        self
    }

/// run_sequential.
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
                        self.target_triple.clone(),
                    )
                })
            })
            .collect()
    }

/// run_parallel.
    pub fn run_parallel(&self, tests: &[Arc<DiscoveredTest>]) -> Vec<TestResult> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.max_concurrent)
            .build()
            .unwrap();
        let tests_c: Vec<Arc<DiscoveredTest>> = tests.to_vec();
        let compiler = Arc::clone(&self.compiler);
        let bless = self.bless;
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
                    "[{}/{}] passed={} failed={} ignored={} :: {}",
                    done, total, passed, failed, ignored, name
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

    if test.config.needs_llvm && std::env::var("GLYIM_LLVM").is_err() {
        return TestResult {
            test,
            revision,
            outcome: TestOutcome::Ignored,
            duration: start.elapsed(),
            diagnostics: Vec::new(),
        };
    }

    if let Some(ref target) = test.config.only_target
        && target != &target_triple
    {
        return TestResult {
            test,
            revision,
            outcome: TestOutcome::Ignored,
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

    TestResult {
        test,
        revision,
        outcome,
        duration,
        diagnostics,
    }
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
    let output = compile_span.in_scope(|| compiler.compile(&test.source, file_id, &flags));

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
        Err(_) => Err(TimeoutError { timeout_secs }),
    }
}
