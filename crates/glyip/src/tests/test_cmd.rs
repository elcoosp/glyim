//! Tests for the `glyip test` command (V20-T03).

use crate::commands::cmd_test;
use crate::config::{NewOptions, TestOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(base: &str) -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{base}-{id}")
}

fn create_test_project(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let result = crate::commands::cmd_new(name, &NewOptions::default(), Some(dir)).expect("new");
    result.path
}

#[test]
fn test_with_only_src_files() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("no-tests");
    let project_path = create_test_project(dir.path(), &name);

    // Remove the tests/ directory entirely.
    let test_dir = project_path.join("tests");
    if test_dir.exists() {
        std::fs::remove_dir_all(&test_dir).expect("remove tests dir");
    }

    let opts = TestOptions {
        no_run: true,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts).expect("test");
    // `src/main.g` contains `fn main()`, which is not a discovered test
    // function, so no tests are counted (and the command still succeeds).
    assert_eq!(result.total, 0, "main() is not a test function");
    assert_eq!(result.failed, 0);
}

#[test]
fn test_with_filter() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("filtered");
    let project_path = create_test_project(dir.path(), &name);

    // A test file containing one real test function.
    std::fs::write(
        project_path.join("tests/match_this.g"),
        "fn test_match_this() {}\n",
    )
    .expect("write test");

    // Filter that matches the discovered function name.
    let opts = TestOptions {
        filter: Some("match_this".to_string()),
        no_run: true,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts).expect("test");
    // The matching test is counted; non-matching tests would be ignored.
    assert_eq!(result.total, 1, "one matching test should be counted");
    assert_eq!(result.ignored, 0);

    // A filter that matches nothing moves the test to the ignored bucket.
    let opts = TestOptions {
        filter: Some("nonexistent".to_string()),
        no_run: true,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts).expect("test");
    assert_eq!(result.total, 0);
    assert_eq!(result.ignored, 1, "non-matching test is ignored");
}

#[test]
fn test_no_run_flag() {
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("no-run");
    let project_path = create_test_project(dir.path(), &name);

    let opts = TestOptions {
        no_run: true,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts);
    assert!(result.is_ok());
}

#[test]
fn test_cmd_test_executes_tests() {
    // S12-T04 / Tier 3.2: `glyip test` must actually compile each test file to
    // MIR and run the discovered test functions via the interpreter — reporting
    // per-test pass/fail and honouring `#[ignore]`.
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("execute");
    let project_path = create_test_project(dir.path(), &name);

    // Overwrite the placeholder test file with passing + ignored tests. A
    // trailing `_pad` function ensures `test_ignored` is not the last-declared
    // function (the MIR pipeline drops the final fn in a file).
    std::fs::write(
        project_path.join("tests/pass.g"),
        "fn test_passes() {}\n\n\
         // #[ignore]\nfn test_ignored() {}\n\n\
         fn _pad() {}\n",
    )
    .expect("write pass tests");

    // A separate file whose only test fails to compile (calls an undefined
    // function). The harness must report it as a failure.
    std::fs::write(
        project_path.join("tests/fail.g"),
        "fn test_fails() { undefined_test_fn_xyz(); }\n\nfn _pad() {}\n",
    )
    .expect("write fail tests");

    let opts = TestOptions {
        no_run: false,
        run_ignored: false,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts).expect("test");

    assert_eq!(result.passed, 1, "one test should pass");
    assert_eq!(result.failed, 1, "one test should fail to compile/run");
    assert_eq!(result.ignored, 1, "the #[ignore] test should be ignored");
    assert_eq!(result.total, 2, "ignored tests are excluded from total");
}

#[test]
fn test_cmd_test_runs_ignored_when_requested() {
    // `--ignored` opt-in runs `#[ignore]` tests too.
    let dir = TempDir::new().expect("temp dir");
    let name = unique_name("run-ignored");
    let project_path = create_test_project(dir.path(), &name);

    std::fs::write(
        project_path.join("tests/integration.g"),
        "fn test_passes() {}\n\n// #[ignore]\nfn test_ignored() {}\n\nfn _pad() {}\n",
    )
    .expect("write tests");

    let opts = TestOptions {
        no_run: false,
        run_ignored: true,
        ..TestOptions::default()
    };
    let result = cmd_test(&project_path, &opts).expect("test");

    assert_eq!(result.passed, 2, "both tests should run and pass");
    assert_eq!(result.ignored, 0, "no test should be ignored");
    assert_eq!(result.total, 2);
}
