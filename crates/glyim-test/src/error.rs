use std::path::PathBuf;

#[derive(Debug)]
/// `TestDiscoveryError`.
pub enum TestDiscoveryError {
    /// The fixture root directory does not exist on disk.
    RootNotFound(PathBuf),
    /// `Variant`.
    ReadFailed {
        /// `Struct`.
        path: PathBuf,
        /// `Struct`.
        source: std::io::Error,
    },
    /// `Variant`.
    InvalidConfig {
        /// `Struct`.
        path: PathBuf,
        /// `Struct`.
        message: String,
    },
    /// `Variant`.
    InvalidAnnotation {
        /// `Struct`.
        path: PathBuf,
        /// `Struct`.
        line: usize,
        /// `Struct`.
        message: String,
    },
    /// `M1.4` `DoD`: the fixture has no `//@` annotation header at all, so it
    /// cannot be joined to a registry feature or backend by the readiness
    /// ledger.
    MissingAnnotations {
        /// `Struct`.
        path: PathBuf,
    },
    /// `M1.4` `DoD`: the fixture is annotated but lacks the expectation its
    /// mode requires (a run-pass/run-fail outcome or a compile-fail error
    /// pattern), so it can only pass by tolerance.
    MissingExpectation {
        /// `Struct`.
        path: PathBuf,
        /// `Struct`.
        mode: crate::harness::config::TestMode,
        /// `Struct`.
        requirement: String,
    },
    /// M1.6/M1.7 strict-xfail witness contract (plan §3.3): the fixture
    /// carries `//@ known-gap: GAP-NNNN` but its resolved mode has no
    /// outcome the inversion can sharpen — the witness contract is
    /// meaningful on `compile-fail` (the compiler rejected / accepted the
    /// witness) and on the run modes (the pinned run behaviour held or
    /// changed, M1.7). `compile-pass` and `ui` pin no run outcome, so a
    /// witness there is annotation drift.
    WitnessUnsupportedMode {
        /// The offending fixture.
        path: PathBuf,
        /// The resolved (non-invertible) mode.
        mode: crate::harness::config::TestMode,
    },
}

impl std::fmt::Display for TestDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotFound(p) => write!(f, "test directory does not exist: {}", p.display()),
            Self::ReadFailed { path, source } => write!(f, "read {}: {source}", path.display()),
            Self::InvalidConfig { path, message } => {
                write!(f, "invalid config in {}: {message}", path.display())
            }
            Self::InvalidAnnotation {
                path,
                line,
                message,
            } => {
                write!(
                    f,
                    "invalid annotation in {} line {line}: {message}",
                    path.display()
                )
            }
            Self::MissingAnnotations { path } => write!(
                f,
                "fixture {} has no `//@` annotation header (plan M1.4: every fixture must be tagged)",
                path.display()
            ),
            Self::MissingExpectation {
                path,
                mode,
                requirement,
            } => write!(
                f,
                "fixture {} ({mode:?}) lacks the required expectation: {requirement}",
                path.display()
            ),
            Self::WitnessUnsupportedMode { path, mode } => write!(
                f,
                "fixture {} declares `//@ known-gap:` but resolves to mode {mode:?}: \
                 a strict-xfail witness must pin an outcome the inversion can sharpen — \
                 `compile-fail` (the compiler rejects or accepts the witness, M1.6) or \
                 `run-pass`/`run-fail` (the pinned run behaviour holds or changes, M1.7) \
                 (plan §3.3). Switch the mode or drop the `//@ known-gap:` header.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TestDiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
/// `FailureReason`.
pub enum FailureReason {
    /// `Variant`.
    CompilePassUnexpectedErrors {
        /// `Struct`.
        errors: Vec<String>,
    },
    /// `Variant`.
    AnnotationParseError {
        /// `Struct`.
        line: usize,
        /// `Struct`.
        message: String,
    },
    /// `Variant`.
    DiagnosticMismatch {
        /// `Struct`.
        missing_count: usize,
        /// `Struct`.
        unexpected_count: usize,
        /// `Struct`.
        wrong_severity_count: usize,
        /// `Struct`.
        details: String,
    },
    /// `Variant`.
    ErrorPatternNotFound {
        /// `Struct`.
        pattern: String,
    },
    /// `Variant`.
    UiOutputDiffers {
        /// `Struct`.
        diff: String,
    },
    /// `Variant`.
    UiNoExpectedFile {
        /// `Struct`.
        path: PathBuf,
    },
    /// Micro-debt fix (M1.6 pass): the `.expected` snapshot file exists
    /// (the `exists()` gate passed) but could not be read — a TOCTOU
    /// window, a permissions change, or an `X.expected` *directory*.
    /// The previous `fs::read_to_string(..).unwrap()` turned this into a
    /// panic that killed the whole test process; it is a per-fixture
    /// failure, not an infra crash.
    UiExpectedUnreadable {
        /// The unreadable snapshot path.
        path: PathBuf,
        /// The underlying io error message.
        message: String,
    },
    /// `Variant`.
    TimeoutExceeded {
        /// `Struct`.
        timeout_secs: u64,
    },
    /// `Variant`.
    CompilationFailed {
        /// `Struct`.
        phase: String,
        /// `Struct`.
        message: String,
    },
    /// TEST-3: a fixture declared an unmet need (`needs-llvm`,
    /// `only-target = …`) and `GLYIM_TEST_NO_SKIP=1` asked the harness to
    /// treat that as a failure rather than silently ignoring the fixture.
    ///
    /// Without the env var, unmet needs produce `TestOutcome::Ignored` —
    /// correct for local development (a macOS dev without LLVM should not
    /// see 60 failures) but wrong for CI, where a fixture that never ran
    /// is a coverage gap pretending to be a pass. `GLYIM_TEST_NO_SKIP=1`
    /// makes that gap visible.
    NeedUnsatisfiable {
        /// The need that was not met, e.g. `"needs-llvm"` or
        /// `"only-target"`.
        need: String,
        /// Human-readable detail: what was required vs. what was
        /// available.
        detail: String,
    },
    /// M1.6 strict-xfail inversion (plan §3.3): a compile-fail fixture
    /// carrying `//@ known-gap: GAP-NNNN` compiled *cleanly* — the gap it
    /// witnesses has closed. This is the strict half of strict-xfail: the
    /// witness must be retired in the same change that fixes the gap, and
    /// the readiness ledger must be updated, otherwise the registry
    /// claims a regression that no longer exists.
    KnownGapClosed {
        /// The witnessed gap ID, e.g. `"GAP-0001"`.
        gap: String,
    },
    /// `Variant`.
    RunFailed {
        /// `Struct`.
        exit_code: Option<i32>,
        /// `Struct`.
        expected_exit_code: Option<i32>,
    },
    /// `Variant`.
    StdoutMismatch {
        /// `Struct`.
        expected: String,
        /// `Struct`.
        actual: String,
    },
    /// `Variant`.
    StderrMismatch {
        /// `Struct`.
        expected: String,
        /// `Struct`.
        actual: String,
    },
    /// `Variant`.
    RunTimeout {
        /// `Struct`.
        timeout_secs: u64,
    },
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompilePassUnexpectedErrors { errors } => {
                write!(
                    f,
                    "expected compilation to succeed, got {} error(s):\n  {}",
                    errors.len(),
                    errors.join("\n  ")
                )
            }
            Self::AnnotationParseError { line, message } => {
                write!(f, "annotation parse error at line {line}: {message}")
            }
            Self::DiagnosticMismatch {
                missing_count,
                unexpected_count,
                wrong_severity_count,
                details,
            } => {
                write!(
                    f,
                    "diagnostic mismatch ({missing_count} missing, {unexpected_count} unexpected, {wrong_severity_count} wrong severity):\n  {details}"
                )
            }
            Self::ErrorPatternNotFound { pattern } => {
                write!(f, "error-pattern '{pattern}' not found")
            }
            Self::UiOutputDiffers { diff } => write!(f, "output differs:\n{diff}"),
            Self::UiNoExpectedFile { path } => write!(f, "no expected file: {}", path.display()),
            Self::UiExpectedUnreadable { path, message } => write!(
                f,
                "cannot read expected snapshot {}: {message}\
                 \nhelp: the file passed the `exists()` check but is unreadable \
                 (removed between the check and the read, a permissions change, \
                 or `X.expected` is a directory). This is a per-fixture failure, \
                 not a harness crash.",
                path.display()
            ),
            Self::TimeoutExceeded { timeout_secs } => {
                write!(f, "exceeded {timeout_secs}s timeout")
            }
            Self::CompilationFailed { phase, message } => {
                write!(f, "compilation failed at {phase}: {message}")
            }
            Self::RunFailed {
                exit_code,
                expected_exit_code,
            } => {
                write!(
                    f,
                    "run failed: exit code {exit_code:?}, expected {expected_exit_code:?}"
                )
            }
            Self::StdoutMismatch { expected, actual } => {
                write!(
                    f,
                    "stdout mismatch:\n  expected: {expected:?}\n  actual:   {actual:?}"
                )
            }
            Self::StderrMismatch { expected, actual } => {
                write!(
                    f,
                    "stderr mismatch:\n  expected: {expected:?}\n  actual:   {actual:?}"
                )
            }
            Self::RunTimeout { timeout_secs } => {
                write!(f, "run exceeded {timeout_secs}s timeout")
            }
            Self::NeedUnsatisfiable { need, detail } => {
                write!(
                    f,
                    "required {need} not satisfied: {detail} (GLYIM_TEST_NO_SKIP=1 makes this a hard failure)"
                )
            }
            Self::KnownGapClosed { gap } => write!(
                f,
                "known-gap {gap} closed: the compiler now accepts this witness fixture.\
                 \nhelp: the strict-xfail contract (plan §3.3, M1.6) requires retiring the \
                 witness in the same change that fixes the gap — remove the \
                 `//@ known-gap:` header, migrate the fixture to the pass corpus, and \
                 update the `[[gap]]` entry under ledger/gaps/**.",
            ),
        }
    }
}

#[derive(Clone, Debug)]
/// `TimeoutError`.
pub struct TimeoutError {
    /// `Struct`.
    pub timeout_secs: u64,
}

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "test exceeded {}s timeout", self.timeout_secs)
    }
}
impl std::error::Error for TimeoutError {}

#[derive(Clone, Debug)]
/// `AssertionFailure`.
pub struct AssertionFailure {
    /// `Struct`.
    pub expected: String,
    /// `Struct`.
    pub actual: String,
    /// `Struct`.
    pub ty_description: String,
}

impl std::error::Error for FailureReason {}
