pub mod artifact;
pub mod collector;
pub mod compiler;
pub mod config;
pub mod display;
pub mod executor;
pub mod flaky;
pub mod harness;
pub mod incremental;
pub mod optimize;
pub mod prioritizer;
pub mod runner;
pub mod snapshot;
pub mod types;
pub mod watcher;

use crate::config::TestConfig;
use crate::display::HumanReporter;
use crate::runner::TestRunner;
use std::path::Path;

pub async fn run_tests(source: &str, config: &TestConfig) -> Vec<crate::types::TestResult> {
    let reporter = HumanReporter;
    let runner = TestRunner::new(config.clone());
    runner.run_all(source, &reporter).await
}

pub async fn run_watch(input: &Path, config: &TestConfig) {
    let mut config = config.clone();
    config.watch = false;
    loop {
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error reading {}: {}", input.display(), e);
                return;
            }
        };
        let _results = run_tests(&source, &config).await;
        if let Ok(watcher) = crate::watcher::FileWatcher::new(
            &[input.to_path_buf()],
            std::time::Duration::from_millis(100),
        ) {
            let _ = watcher.wait_for_change();
        }
    }
}

pub fn run_tests_sync(source: &str, config: &TestConfig) -> std::vec::Vec<types::TestResult> {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error creating tokio runtime: {}", e);
            return vec![];
        }
    };
    rt.block_on(async {
        // If compilation or test collection fails, return empty
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let rt_local = tokio::runtime::Runtime::new().unwrap();
            rt_local.block_on(run_tests(source, config))
        })) {
            Ok(res) => res,
            Err(_) => vec![],
        }
    })
}
