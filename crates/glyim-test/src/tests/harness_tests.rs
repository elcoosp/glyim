use crate::*;

#[test]
fn test_config_parsing() {
    let source = "// test-mode: compile-fail\n// error-pattern: mismatched types\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert!(result.has_explicit_mode);
    assert_eq!(result.config.mode, harness::config::TestMode::CompileFail);
    assert_eq!(result.config.error_patterns.len(), 1);
    assert_eq!(result.config.error_patterns[0], "mismatched types");
}

#[test]
fn test_config_default_mode() {
    let source = "fn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert!(!result.has_explicit_mode);
    assert_eq!(result.config.mode, harness::config::TestMode::CompilePass);
}

#[test]
fn test_config_ignore() {
    let source = "// ignore\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert!(result.config.ignore);
}

#[test]
fn test_config_timeout() {
    let source = "// timeout: 120\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert_eq!(result.config.timeout_secs, 120);
}

#[test]
fn test_config_compile_flags() {
    let source = "// compile-flags: --emit=mir -Zdump-mir=all\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert_eq!(result.config.compile_flags.len(), 2);
}

#[test]
fn test_config_revisions() {
    let source = "// revisions: a b c\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert_eq!(result.config.revisions, vec!["a", "b", "c"]);
}

#[test]
fn test_config_revision_flags() {
    let source = "// revisions: a b\n//[a] compile-flags: -Dwarnings\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert!(result.config.revision_compile_flags.contains_key("a"));
}

#[test]
fn test_test_mode_from_str() {
    assert_eq!(
        "compile-pass".parse::<harness::config::TestMode>().unwrap(),
        harness::config::TestMode::CompilePass
    );
    assert_eq!(
        "compile-fail".parse::<harness::config::TestMode>().unwrap(),
        harness::config::TestMode::CompileFail
    );
    assert_eq!(
        "ui".parse::<harness::config::TestMode>().unwrap(),
        harness::config::TestMode::Ui
    );
    assert!("invalid".parse::<harness::config::TestMode>().is_err());
}

#[test]
fn test_test_mode_dir_name() {
    assert_eq!(
        harness::config::TestMode::CompilePass.dir_name(),
        "compile-pass"
    );
    assert_eq!(
        harness::config::TestMode::CompileFail.dir_name(),
        "compile-fail"
    );
    assert_eq!(harness::config::TestMode::Ui.dir_name(), "ui");
}

#[test]
fn test_run_pass_mode_from_str() {
    assert_eq!(
        "run-pass".parse::<harness::config::TestMode>().unwrap(),
        harness::config::TestMode::RunPass
    );
    assert_eq!(
        "run-fail".parse::<harness::config::TestMode>().unwrap(),
        harness::config::TestMode::RunFail
    );
}

#[test]
fn test_run_pass_mode_dir_name() {
    assert_eq!(harness::config::TestMode::RunPass.dir_name(), "run-pass");
    assert_eq!(harness::config::TestMode::RunFail.dir_name(), "run-fail");
}

#[test]
fn test_config_check_stdout() {
    let source = "// test-mode: run-pass\n// check-stdout: Hello, world!\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert_eq!(result.config.mode, harness::config::TestMode::RunPass);
    assert_eq!(result.config.check_stdout.as_deref(), Some("Hello, world!"));
}

#[test]
fn test_config_check_stderr() {
    let source = "// test-mode: run-fail\n// check-stderr: panic\n// exit-code: 101\nfn main() {}";
    let result = harness::config::parse_test_config(source).unwrap();
    assert_eq!(result.config.mode, harness::config::TestMode::RunFail);
    assert_eq!(result.config.check_stderr.as_deref(), Some("panic"));
    assert_eq!(result.config.expected_exit_code, Some(101));
}

#[test]
fn test_output_check_exit_code_pass() {
    let check = harness::runner::OutputCheck::new().exit_code(0);
    let result = harness::runner::RunResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_ok());
}

#[test]
fn test_output_check_exit_code_fail() {
    let check = harness::runner::OutputCheck::new().exit_code(0);
    let result = harness::runner::RunResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_err());
}

#[test]
fn test_output_check_stdout_pass() {
    let check = harness::runner::OutputCheck::new().stdout("hello");
    let result = harness::runner::RunResult {
        exit_code: Some(0),
        stdout: "say hello world".to_string(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_ok());
}

#[test]
fn test_output_check_stdout_fail() {
    let check = harness::runner::OutputCheck::new().stdout("goodbye");
    let result = harness::runner::RunResult {
        exit_code: Some(0),
        stdout: "say hello world".to_string(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_err());
}

#[test]
fn test_output_check_stderr_pass() {
    let check = harness::runner::OutputCheck::new().stderr("error:");
    let result = harness::runner::RunResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "error: something went wrong".to_string(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_ok());
}

#[test]
fn test_output_check_timeout() {
    let check = harness::runner::OutputCheck::new().exit_code(0);
    let result = harness::runner::RunResult {
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: true,
        duration: std::time::Duration::from_secs(60),
    };
    let err = check.check(&result).unwrap_err();
    assert!(matches!(err, error::FailureReason::RunTimeout { .. }));
}

#[test]
fn test_output_check_run_fail_pass() {
    let check = harness::runner::OutputCheck::new()
        .exit_code(1)
        .stderr("panic");
    let result = harness::runner::RunResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "thread panicked: panic at core.rs:42".to_string(),
        timed_out: false,
        duration: std::time::Duration::from_millis(100),
    };
    assert!(check.check(&result).is_ok());
}

#[test]
fn test_program_runner_nonexistent() {
    let runner = harness::runner::ProgramRunner::new("/nonexistent/program");
    let result = runner.run(std::time::Duration::from_secs(5));
    assert!(result.exit_code.is_none());
    assert!(!result.stderr.is_empty());
}

#[test]
fn test_program_runner_echo() {
    let echo_path = "/bin/echo";
    let runner = harness::runner::ProgramRunner::new(echo_path).arg("hello world");
    let result = runner.run(std::time::Duration::from_secs(5));
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("hello world"));
}

#[test]
fn test_program_runner_false() {
    let false_path = if cfg!(target_os = "macos") {
        "/usr/bin/false"
    } else {
        "/bin/false"
    };
    let runner = harness::runner::ProgramRunner::new(false_path);
    let result = runner.run(std::time::Duration::from_secs(5));
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, Some(1));
}

#[test]
fn test_program_runner_with_stdin() {
    let cat_path = "/bin/cat";
    let runner = harness::runner::ProgramRunner::new(cat_path).stdin("input data");
    let result = runner.run(std::time::Duration::from_secs(5));
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("input data"));
}

#[test]
fn test_program_runner_kills_child_on_timeout() {
    // A deliberately-hanging child (sleep 30) with a 1s timeout must report
    // timed_out and return promptly — the child process must be killed (§24.2),
    // not left running as an orphan until its full duration elapses.
    let sleep_path = "/bin/sleep";
    let start = std::time::Instant::now();
    let runner = harness::runner::ProgramRunner::new(sleep_path).arg("30");
    let result = runner.run(std::time::Duration::from_secs(1));
    let elapsed = start.elapsed();

    assert!(result.timed_out, "timeout must be reported");
    // The child must be killed: the run should finish well before the 30s the
    // child would otherwise sleep. Allow generous slack for CI slowness.
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "child was not killed on timeout (elapsed {:?})",
        elapsed
    );
}

#[test]
fn test_pipeline_compiler_surfaces_mir_artifacts() {
    use crate::harness::compiler::TestCompiler;
    use crate::mock::MockCodegen;
    use glyim_span::FileId;
    use std::sync::Arc;

    // Tier 7.1: the full-pipeline compiler must populate CompileOutput with
    // the MIR bodies/def-map/typeck result it used to discard, and must write
    // its object file to a per-file temp path (not a shared "test_output.o").
    let mock = Arc::new(MockCodegen::new());
    let backend: Arc<dyn glyim_codegen::CodegenBackend + Send + Sync> = mock.clone();
    let compiler = harness::compiler::PipelineCompiler::new(backend.clone());

    let source = "fn main() {}";
    let output = compiler.compile(source, FileId::from_raw(777), &[]);

    assert!(
        output.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.def_map.is_some(),
        "def_map must be populated, was None"
    );
    assert!(
        output.typeck_result.is_some(),
        "typeck_result must be populated, was None"
    );
    assert!(
        !output.mir_bodies.is_empty(),
        "mir_bodies must be populated (pipeline used to discard them)"
    );

    // The mock backend recorded exactly one generate call, to a per-file temp
    // path (the path embeds the file id `777`, proving it is not a shared
    // "test_output.o"; it is also uniquified per call to avoid collisions
    // between concurrently-running tests that reuse the same file id).
    let calls = mock.calls();
    assert_eq!(calls.len(), 1, "expected one codegen generate call");
    let out = &calls[0].output_path;
    assert!(
        out.to_string_lossy().contains("777.o"),
        "expected per-file temp path, got {:?}",
        out
    );
}

#[test]
fn test_run_pass_strategy_executes_provided_executable() {
    use crate::harness::compiler::CompileOutput;
    use crate::harness::config::TestConfig;
    use crate::harness::strategy::RunPassStrategy;
    use std::path::Path;
    use std::time::Duration;

    // Tier 7.2: when a real executable is provided (the linking step in
    // PipelineCompiler::compile produces one from a real object file), the
    // run-pass strategy must actually execute it and check its output.
    // Here we drive the consumer side directly with /bin/echo so the test is
    // deterministic regardless of whether the mock backend can emit a linkable
    // object on this platform. We assert the process ran and exited 0 (a
    // nonexistent exe would yield a spawn failure / "no executable produced"
    // instead), proving the executor now actually runs the provided path.
    let config = TestConfig::default();
    let output = CompileOutput {
        diagnostics: Vec::new(),
        syntax_tree: None,
        def_map: None,
        typeck_result: None,
        mir_bodies: Vec::new(),
        ty_ctx: None,
        executable_path: Some(Path::new("/bin/echo").to_path_buf()),
    };

    let outcome = RunPassStrategy {}.evaluate(
        &output,
        "fn main() {}",
        output.executable_path.as_deref(),
        &config,
        Duration::from_secs(5),
    );
    assert!(
        matches!(outcome, crate::harness::executor::TestOutcome::Passed),
        "run-pass with /bin/echo should pass, got {:?}",
        outcome
    );
}

#[test]
fn test_run_pass_strategy_no_executable_fails() {
    use crate::harness::compiler::CompileOutput;
    use crate::harness::config::TestConfig;
    use crate::harness::strategy::RunPassStrategy;
    use std::time::Duration;

    // Without an executable (e.g. mock backend emitted no real object and the
    // link step fell back to None), run-pass must report "no executable
    // produced" rather than silently passing.
    let output = CompileOutput {
        diagnostics: Vec::new(),
        syntax_tree: None,
        def_map: None,
        typeck_result: None,
        mir_bodies: Vec::new(),
        ty_ctx: None,
        executable_path: None,
    };
    let outcome = RunPassStrategy {}.evaluate(
        &output,
        "fn main() {}",
        None,
        &TestConfig::default(),
        Duration::from_secs(5),
    );
    assert!(
        matches!(
            outcome,
            crate::harness::executor::TestOutcome::Failed {
                reason: crate::FailureReason::CompilationFailed { .. }
            }
        ),
        "run-pass with no exe must fail, got {:?}",
        outcome
    );
}

#[test]
fn test_pipeline_compiler_populates_executable_path_field() {
    use crate::harness::compiler::TestCompiler;
    use crate::mock::MockCodegen;
    use glyim_span::FileId;
    use std::sync::Arc;

    // Tier 7.2: the CompileOutput produced by PipelineCompiler must now carry
    // the `executable_path` field. With the mock backend (which emits no real
    // object), the link step cannot succeed, so the field is None — but the
    // value must be present and the linking path must not panic. (A real
    // backend producing a linkable object would populate Some(...).)
    let backend: Arc<dyn glyim_codegen::CodegenBackend + Send + Sync> =
        Arc::new(MockCodegen::new());
    let compiler = crate::harness::compiler::PipelineCompiler::new(backend);
    let output = compiler.compile("fn main() {}", FileId::from_raw(888), &[]);
    assert!(
        output.executable_path.is_none(),
        "mock backend emits no real object, so linking yields None (got {:?})",
        output.executable_path
    );
}

#[test]
fn test_run_parallel_collects_results_with_progress_reporting() {
    use crate::harness::collector::DiscoveredTest;
    use crate::harness::config::TestMode;
    use crate::harness::executor::{TestExecutor, TestOutcome};

    // Regression guard for plan §24.1: run_parallel must still return the same
    // results as run_sequential (the progress-reporting channel must not drop
    // or reorder any result), and it must report progress for every task.
    let mk = |name: &str| {
        let t = DiscoveredTest {
            path: std::path::PathBuf::from("."),
            name: name.to_string(),
            source: std::sync::Arc::from("fn main() {}"),
            config: crate::harness::config::TestConfig {
                mode: TestMode::CompilePass,
                ..Default::default()
            },
            revisions: vec!["base".to_string()],
        };
        std::sync::Arc::new(t)
    };
    let tests = vec![mk("alpha"), mk("beta"), mk("gamma")];

    let executor = TestExecutor::new(
        std::time::Duration::from_secs(30),
        false,
        false,
        4,
        false, // frontend-only compiler (no LLVM needed)
    );

    let results = executor.run_parallel(&tests);
    assert_eq!(results.len(), 3, "all three tests must be collected");
    for r in &results {
        assert!(
            matches!(r.outcome, TestOutcome::Passed),
            "test {} should pass, got {:?}",
            r.test.name,
            r.outcome
        );
    }

    // Same input via sequential path must agree (proves the reporter side-channel
    // did not alter collected outcomes).
    let seq = executor.run_sequential(&tests);
    assert_eq!(seq.len(), results.len());
    for (a, b) in seq.iter().zip(results.iter()) {
        assert_eq!(a.test.name, b.test.name);
        assert!(matches!(a.outcome, TestOutcome::Passed));
    }
}
