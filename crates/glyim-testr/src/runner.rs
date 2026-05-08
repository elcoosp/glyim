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

    pub async fn run_all(&self, source: &str, display: &dyn DisplayBackend) -> Vec<TestResult> {
        // If incremental mode, check cache first
        let _cached_results: Vec<crate::types::TestResult> = Vec::new();
        let source_hash = glyim_macro_vfs::ContentHash::of(source.as_bytes());
        let cache = if self.config.incremental {
            crate::cache::IncrementalTestCache::new(&std::path::PathBuf::from(
                ".glyim/incremental/test-cache",
            ))
        } else {
            None
        };

        let artifact = match if self.config.coverage {
            Compiler::compile_with_opts(source, self.config.filter.as_deref(), true)
        } else {
            Compiler::compile(source, self.config.filter.as_deref())
        } {
            Ok(a) => a,
            Err(e) => {
                if matches!(e, crate::compiler::CompileError::NoTests) {
                    display.suite_started(0);
                    display.suite_finished(0, 0, 0);
                    return vec![];
                }
                let errname = e.to_string();
                display.suite_started(0);
                display.suite_finished(0, 0, 0);
                return vec![TestResult {
                    name: errname,
                    outcome: crate::types::TestOutcome::CompilationError(e.to_string()),
                    duration: Duration::ZERO,
                }];
            }
        };

        let num_tests = artifact.test_defs.len();
        display.suite_started(num_tests);

        // If single binary, run each test against that binary
        if let Some(ref single_bin) = artifact.bin_path {
            let mut set: JoinSet<Result<TestResult, String>> = JoinSet::new();
            for test_def in &artifact.test_defs {
                let name = test_def.name.clone();
                let bin = single_bin.clone();
                let timeout = Duration::from_secs(self.config.timeout_secs);
                let should_panic = test_def.should_panic;
                display.test_started(&name);
                set.spawn(async move {
                    let exec = Executor::new(bin, timeout);
                    let mut result = exec.run_test(&name).await?;
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
            while let Some(r) = set.join_next().await {
                match r {
                    Ok(Ok(tr)) => {
                        display.test_finished(&tr);
                        results.push(tr);
                    }
                    Ok(Err(e)) => {
                        let tr = TestResult {
                            name: "error".into(),
                            outcome: crate::types::TestOutcome::CompilationError(e),
                            duration: Duration::ZERO,
                        };
                        display.test_finished(&tr);
                        results.push(tr);
                    }
                    Err(je) => {
                        let tr = TestResult {
                            name: "panic".into(),
                            outcome: crate::types::TestOutcome::CompilationError(format!("{je}")),
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
            return results;
        }

        // Multiple binaries: map test name to binary
        let binary_map: std::collections::HashMap<&str, &std::path::Path> = artifact
            .per_test_binaries
            .iter()
            .map(|(name, path)| (name.as_str(), path.as_ref()))
            .collect();
        let mut set: JoinSet<Result<TestResult, String>> = JoinSet::new();
        for test_def in &artifact.test_defs {
            let name = test_def.name.clone();
            let should_panic = test_def.should_panic;
            let timeout = Duration::from_secs(self.config.timeout_secs);
            display.test_started(&name);
            let bin_path = match binary_map.get(name.as_str()) {
                Some(p) => p.to_path_buf(),
                None => continue,
            };
            set.spawn(async move {
                let exec = Executor::new(bin_path, timeout);
                let mut result = exec.run_test(&name).await?;
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
        while let Some(r) = set.join_next().await {
            match r {
                Ok(Ok(tr)) => {
                    display.test_finished(&tr);
                    results.push(tr);
                }
                Ok(Err(e)) => {
                    let tr = TestResult {
                        name: "error".into(),
                        outcome: crate::types::TestOutcome::CompilationError(e),
                        duration: Duration::ZERO,
                    };
                    display.test_finished(&tr);
                    results.push(tr);
                }
                Err(je) => {
                    let tr = TestResult {
                        name: "panic".into(),
                        outcome: crate::types::TestOutcome::CompilationError(format!("{je}")),
                        duration: Duration::ZERO,
                    };
                    display.test_finished(&tr);
                    results.push(tr);
                }
            }
        }

        // Store results in cache if incremental mode
        if let Some(ref c) = cache {
            for r in &results {
                c.store_result(r, &source_hash);
            }
        }

        let passed = results
            .iter()
            .filter(|r| matches!(r.outcome, crate::types::TestOutcome::Passed))
            .count();
        let failed = results.len() - passed;
        // Coverage: merge per-test dumps if coverage enabled
        if self.config.coverage {
            let cov_dir = std::path::Path::new("glyim-cov");
            let _ = std::fs::create_dir_all(cov_dir);
            let mut merged_dump = glyim_coverage::data::CoverageDump {
                files: std::collections::HashMap::new(),
                counters: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
                version: 1,
            };
            for entry in std::fs::read_dir(cov_dir)
                .unwrap_or_else(|_| {
                    std::fs::read_dir(".").unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
                })
                .flatten()
            {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json")
                    && let Ok(data) = std::fs::read_to_string(&path)
                    && let Ok(dump) =
                        serde_json::from_str::<glyim_coverage::data::CoverageDump>(&data)
                {
                    merged_dump.merge(&dump);
                }
            }
            let merged_path = cov_dir.join("merged.json");
            if let Ok(json) = serde_json::to_string(&merged_dump) {
                let _ = std::fs::write(&merged_path, &json);
                eprintln!("Coverage report: {}", merged_path.display());
            }
        }

        display.suite_finished(passed, failed, results.len());
        results
    }
}
