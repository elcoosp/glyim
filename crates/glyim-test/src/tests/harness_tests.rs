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
    // T196-PATCHED [HARNESS-9]: the stdout oracle is line-anchored, so the
    // expected text must match a whole line rather than being a substring.
    let check = harness::runner::OutputCheck::new().stdout("hello");
    let result = harness::runner::RunResult {
        exit_code: Some(0),
        stdout: "hello\n".to_string(),
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
        stdout: "hello\n".to_string(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(check.check(&result).is_err());
}

#[test]
fn test_output_check_stdout_substring_no_longer_matches() {
    // T196-REGRESSION: verify the line-anchored semantics — "1" no longer
    // matches "123" via substring.
    let check = harness::runner::OutputCheck::new().stdout("1");
    let result = harness::runner::RunResult {
        exit_code: Some(0),
        stdout: "123\n".to_string(),
        stderr: String::new(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(
        check.check(&result).is_err(),
        "line-anchored check must reject substring matches"
    );
}

#[test]
fn test_output_check_stderr_pass() {
    // TEST-1: `check-stderr` is line-anchored. A prefix expectation
    // (`"error:"`) no longer matches `"error: something went wrong"`;
    // the whole line must be given.
    let check = harness::runner::OutputCheck::new().stderr("error: something went wrong");
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
fn test_output_check_stderr_prefix_is_not_a_match() {
    // Guards against a future refactor reverting the matcher to
    // `.contains`.
    let check = harness::runner::OutputCheck::new().stderr("error:");
    let result = harness::runner::RunResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "error: something went wrong".to_string(),
        timed_out: false,
        duration: std::time::Duration::from_secs(0),
    };
    assert!(
        check.check(&result).is_err(),
        "prefix match must not satisfy a line-anchored stderr expectation"
    );
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
    // TEST-1: full line, not the substring `"panic"`. The
    // `exit_code(1)` side is unchanged.
    let check = harness::runner::OutputCheck::new()
        .exit_code(1)
        .stderr("thread panicked: panic at core.rs:42");
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
fn test_output_check_stderr_substring_is_not_a_match() {
    // A bare word inside a longer line must not satisfy a line-anchored
    // expectation.
    let check = harness::runner::OutputCheck::new().stderr("panic");
    let result = harness::runner::RunResult {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: "thread panicked: panic at core.rs:42".to_string(),
        timed_out: false,
        duration: std::time::Duration::from_millis(100),
    };
    assert!(
        check.check(&result).is_err(),
        "substring match must not satisfy a line-anchored stderr expectation"
    );
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
        "child was not killed on timeout (elapsed {elapsed:?})",
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
    let output = compiler.compile(
        source,
        std::path::Path::new("test.g"),
        FileId::from_raw(777),
        &[],
    );

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
        "expected per-file temp path, got {out:?}",
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
        executable_cleanup: None,
        entry_main: None,
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
        "run-pass with /bin/echo should pass, got {outcome:?}",
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
        executable_cleanup: None,
        entry_main: None,
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
        "run-pass with no exe must fail, got {outcome:?}",
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
    let output = compiler.compile(
        "fn main() {}",
        std::path::Path::new("test.g"),
        FileId::from_raw(888),
        &[],
    );
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
        false, // bless
        false, // no_skip
        4, false, // frontend-only compiler (no LLVM needed)
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

// -------------------------------------------------------------------------
// M1.6 strict-xfail witness (plan §3.3): `//@ known-gap: GAP-NNNN` on a
// compile-fail fixture inverts the failure meaning — the compiler *still
// rejecting* the witness is the pass state, and the compiler *accepting* it
// means the gap has closed and the witness must be retired.
// -------------------------------------------------------------------------

#[test]
fn witness_reports_known_gap_closed_when_compiler_accepts() {
    use crate::harness::collector::DiscoveredTest;
    use crate::harness::compiler::FrontendOnlyCompiler;
    use crate::harness::config::{TestConfig, TestMode};
    use crate::harness::executor::{TestExecutor, TestOutcome};

    // A witness fixture whose source compiles cleanly (no diagnostics at
    // all) and whose compile-fail expectations can therefore not match:
    // plain compile-fail would report `DiagnosticMismatch`; the witness
    // seam must sharpen that into `KnownGapClosed`.
    let t = DiscoveredTest {
        path: std::path::PathBuf::from("witness.g"),
        name: "witness".to_string(),
        source: std::sync::Arc::from("fn main() {}\n"),
        config: TestConfig {
            mode: TestMode::CompileFail,
            known_gap: Some("GAP-0001".to_string()),
            ..Default::default()
        },
        revisions: vec!["default".to_string()],
    };

    let executor = TestExecutor::new(false, false, 1, false)
        .with_compiler(std::sync::Arc::new(FrontendOnlyCompiler));
    let results = executor.run_sequential(&[std::sync::Arc::new(t)]);

    assert_eq!(results.len(), 1);
    match &results[0].outcome {
        TestOutcome::Failed { reason } => match reason {
            crate::error::FailureReason::KnownGapClosed { gap } => {
                assert_eq!(gap, "GAP-0001");
            }
            other => panic!("expected KnownGapClosed, got {other:?}"),
        },
        other => panic!("expected Failed outcome, got {other:?}"),
    }
}

#[test]
fn witness_without_known_gap_reports_plain_mismatch() {
    use crate::harness::collector::DiscoveredTest;
    use crate::harness::compiler::FrontendOnlyCompiler;
    use crate::harness::config::{TestConfig, TestMode};
    use crate::harness::executor::{TestExecutor, TestOutcome};

    // Control: a fixture under plain compile-fail (no `//@ known-gap:`) that
    // fails to satisfy its inline annotation keeps the generic
    // diagnostic-mismatch failure — the witness sharpening must only apply
    // to real witnesses, not to ordinary compile-fail bookkeeping.
    let t = DiscoveredTest {
        path: std::path::PathBuf::from("plain.g"),
        name: "plain".to_string(),
        // Parse-only compiler: the type error never fires, so the inline
        // annotation is unmet → DiagnosticMismatch.
        source: std::sync::Arc::from(
            "fn main() {\n    let x: i32 = \"hello\"; //~ ERROR mismatched types\n}\n",
        ),
        config: TestConfig {
            mode: TestMode::CompileFail,
            known_gap: None,
            ..Default::default()
        },
        revisions: vec!["default".to_string()],
    };

    let executor = TestExecutor::new(false, false, 1, false)
        .with_compiler(std::sync::Arc::new(FrontendOnlyCompiler));
    let results = executor.run_sequential(&[std::sync::Arc::new(t)]);

    match &results[0].outcome {
        TestOutcome::Failed {
            reason: crate::error::FailureReason::DiagnosticMismatch { .. },
        } => {}
        other => panic!("expected DiagnosticMismatch, got {other:?}"),
    }
}

#[test]
fn collector_rejects_known_gap_on_non_compile_fail_modes() {
    use crate::error::TestDiscoveryError;
    use crate::harness::collector::TestCollector;
    use std::fs;

    // A `//@ known-gap:` witness must be a compile-fail fixture: the
    // inversion is defined on "the compiler rejected / accepted the
    // witness", which only compile-fail measures. Any other mode is drift
    // and is rejected at discovery, not silently tolerated.
    for (mode_dir, header) in [
        ("run-pass", "//@ mode: run-pass\n//@ expect-exit: 0\n"),
        ("ui", "//@ mode: ui\n"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join(mode_dir);
        fs::create_dir(&root).unwrap();
        let fixture = root.join("witness.g");
        fs::write(
            &fixture,
            format!("{header}//@ known-gap: GAP-0001\nfn main() {{}}\n"),
        )
        .unwrap();

        let collector = TestCollector::new(dir.path());
        let err = collector
            .collect(None, None)
            .expect_err("known-gap on a non-compile-fail mode must be rejected");
        match err {
            TestDiscoveryError::WitnessRequiresCompileFail { path, mode } => {
                assert_eq!(path, fixture);
                assert_ne!(
                    mode,
                    crate::harness::config::TestMode::CompileFail,
                    "the reported mode must be the offending one"
                );
            }
            other => panic!("expected WitnessRequiresCompileFail, got {other:?}"),
        }
    }
}

#[test]
fn ui_unreadable_expected_snapshot_is_a_typed_failure() {
    // Micro-debt fix (M1.6 pass): `UiTestStrategy::evaluate` used
    // `fs::read_to_string(..).unwrap()` right after an `exists()` check.
    // The TOCTOU window (file removed/permissions changed in between) or
    // an `X.expected` *directory* crashed the whole test process instead
    // of reporting a typed failure.
    use crate::harness::compiler::CompileOutput;
    use crate::harness::strategy::UiTestStrategy;

    let dir = tempfile::tempdir().expect("tempdir");
    let test_path = dir.path().join("case.g");
    std::fs::write(&test_path, "fn main() {}\n").unwrap();
    // `case.expected` exists but is a *directory*: exists() is true,
    // read_to_string fails. The old `.unwrap()` panicked here.
    std::fs::create_dir(dir.path().join("case.expected")).unwrap();

    let outcome =
        UiTestStrategy.evaluate(&CompileOutput::default(), "fn main() {}", &test_path, false);
    match outcome {
        crate::harness::executor::TestOutcome::Failed {
            reason: crate::error::FailureReason::UiExpectedUnreadable { path, message },
        } => {
            assert_eq!(path, dir.path().join("case.expected"));
            assert!(
                message.contains("Is a directory") || !message.is_empty(),
                "the io error message must be carried through: {message}"
            );
        }
        other => panic!("expected UiExpectedUnreadable failure, got {other:?}"),
    }
}

/// Micro-debt cleanup (M2.1 companion): a fixture that mixes a legacy
/// `// key:` header with its canonical `//@` spelling fails with a
/// **typed** error carrying miette help text — not the bare `String` the
/// seam used to return, which callers could only embed in prose.
#[test]
fn test_config_conflict_error_is_typed_with_help() {
    use miette::Diagnostic as _;
    let source = "// test-mode: run-pass\n//@ mode: run-pass\nfn main() {}";
    let err = harness::config::parse_test_config(source).expect_err("conflict must fail");
    assert!(
        err.message.contains("legacy"),
        "message names the conflict: {}",
        err.message
    );
    let help_text = err
        .help()
        .expect("typed error carries miette help")
        .to_string();
    assert!(
        help_text.contains("//@"),
        "help points at the //@ grammar: {help_text}"
    );
    // It renders through miette like every harness diagnostic.
    let rendered = format!("{:?}", miette::Report::new(err));
    assert!(
        rendered.contains("legacy"),
        "miette report renders: {rendered}"
    );
}
