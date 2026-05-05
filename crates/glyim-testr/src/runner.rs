use crate::compiler::Compiler;
use crate::config::TestConfig;
use crate::display::DisplayBackend;
use crate::executor::Executor;
use crate::types::TestResult;
use std::time::Duration;
use tokio::task::JoinSet;

pub struct TestRunner {
    config: TestConfig,
}

impl TestRunner {
    pub fn new(config: TestConfig) -> Self {
        Self { config }
    }

    pub async fn run_all(
        &self,
        source: &str,
        display: &dyn DisplayBackend,
    ) -> Vec<TestResult> {
        let artifact = match Compiler::compile(source) {
            Ok(a) => a,
            Err(e) => {
                let errname = match &e {
                    crate::compiler::CompileError::NoTests => "no tests".into(),
                    other => other.to_string(),
                };
                display.suite_started(0);
                display.suite_finished(0, 0, 0);
                return vec![TestResult {
                    name: errname,
                    outcome: crate::types::TestOutcome::CompilationError(e.to_string()),
                    duration: Duration::ZERO,
                }];
            }
        };

        // Filter tests if config has a filter
        let test_defs: Vec<&crate::types::TestDef> = if let Some(ref filter) = self.config.filter {
            artifact.test_defs.iter().filter(|t| t.name == *filter).collect()
        } else {
            artifact.test_defs.iter().collect()
        };

        display.suite_started(test_defs.len());

        let mut set: JoinSet<Result<TestResult, String>> = JoinSet::new();

        for test_def in &test_defs {
            let name = test_def.name.clone();
            let bin_path = artifact.bin_path.clone();
            let timeout = Duration::from_secs(self.config.timeout_secs);
            let should_panic = test_def.should_panic;
            display.test_started(&name);

            set.spawn(async move {
                let exec = Executor::new(bin_path, timeout);
                let mut result = exec.run_test(&name).await?;
                // Adjust outcome for should_panic
                if should_panic {
                    match result.outcome {
                        crate::types::TestOutcome::Failed { .. } => {
                            result.outcome = crate::types::TestOutcome::Passed;
                        }
                        crate::types::TestOutcome::Passed => {
                            result.outcome = crate::types::TestOutcome::Failed {
                                exit_code: 0,
                                stderr: String::new(),
                            };
                        }
                        _ => {}
                    }
                }
                Ok(result)
            });
        }

        let mut results = Vec::new();

        while let Some(result) = set.join_next().await {
            match result {
                Ok(Ok(test_result)) => {
                    display.test_finished(&test_result);
                    results.push(test_result);
                }
                Ok(Err(e)) => {
                    let tr = TestResult {
                        name: "error".into(),
                        outcome: crate::types::TestOutcome::CompilationError(e.to_string()),
                        duration: Duration::ZERO,
                    };
                    display.test_finished(&tr);
                    results.push(tr);
                }
                Err(join_error) => {
                    let tr = TestResult {
                        name: "panic".into(),
                        outcome: crate::types::TestOutcome::CompilationError(format!(
                            "task panicked: {}",
                            join_error
                        )),
                        duration: Duration::ZERO,
                    };
                    display.test_finished(&tr);
                    results.push(tr);
                }
            }
        }

        let passed = results
            .iter()
            .filter(|r| matches!(r.outcome, crate::types::TestOutcome::Passed))
            .count();
        let failed = results.len() - passed;
        display.suite_finished(passed, failed, results.len());
        results
    }
}
