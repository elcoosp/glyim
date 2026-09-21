use std::path::PathBuf;

#[derive(Debug)]
/// TestDiscoveryError.
pub enum TestDiscoveryError {
    #[allow(missing_docs)]
    RootNotFound(PathBuf),
    /// Variant.
    ReadFailed {
        /// Struct.
        path: PathBuf,
        /// Struct.
        source: std::io::Error,
    },
    /// Variant.
    InvalidConfig {
        /// Struct.
        path: PathBuf,
        /// Struct.
        message: String,
    },
    /// Variant.
    InvalidAnnotation {
        /// Struct.
        path: PathBuf,
        /// Struct.
        line: usize,
        /// Struct.
        message: String,
    },
}

impl std::fmt::Display for TestDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotFound(p) => write!(f, "test directory does not exist: {:?}", p),
            Self::ReadFailed { path, source } => write!(f, "read {:?}: {}", path, source),
            Self::InvalidConfig { path, message } => {
                write!(f, "invalid config in {:?}: {}", path, message)
            }
            Self::InvalidAnnotation {
                path,
                line,
                message,
            } => {
                write!(
                    f,
                    "invalid annotation in {:?} line {}: {}",
                    path, line, message
                )
            }
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
/// FailureReason.
pub enum FailureReason {
    /// Variant.
    CompilePassUnexpectedErrors {
        /// Struct.
        errors: Vec<String>,
    },
    /// Variant.
    AnnotationParseError {
        /// Struct.
        line: usize,
        /// Struct.
        message: String,
    },
    /// Variant.
    DiagnosticMismatch {
        /// Struct.
        missing_count: usize,
        /// Struct.
        unexpected_count: usize,
        /// Struct.
        wrong_severity_count: usize,
        /// Struct.
        details: String,
    },
    /// Variant.
    ErrorPatternNotFound {
        /// Struct.
        pattern: String,
    },
    /// Variant.
    UiOutputDiffers {
        /// Struct.
        diff: String,
    },
    /// Variant.
    UiNoExpectedFile {
        /// Struct.
        path: PathBuf,
    },
    /// Variant.
    TimeoutExceeded {
        /// Struct.
        timeout_secs: u64,
    },
    /// Variant.
    CompilationFailed {
        /// Struct.
        phase: String,
        /// Struct.
        message: String,
    },
    /// Variant.
    RunFailed {
        /// Struct.
        exit_code: Option<i32>,
        /// Struct.
        expected_exit_code: Option<i32>,
    },
    /// Variant.
    StdoutMismatch {
        /// Struct.
        expected: String,
        /// Struct.
        actual: String,
    },
    /// Variant.
    StderrMismatch {
        /// Struct.
        expected: String,
        /// Struct.
        actual: String,
    },
    /// Variant.
    RunTimeout {
        /// Struct.
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
                write!(f, "annotation parse error at line {}: {}", line, message)
            }
            Self::DiagnosticMismatch {
                missing_count,
                unexpected_count,
                wrong_severity_count,
                details,
            } => {
                write!(
                    f,
                    "diagnostic mismatch ({} missing, {} unexpected, {} wrong severity):\n  {}",
                    missing_count, unexpected_count, wrong_severity_count, details
                )
            }
            Self::ErrorPatternNotFound { pattern } => {
                write!(f, "error-pattern '{}' not found", pattern)
            }
            Self::UiOutputDiffers { diff } => write!(f, "output differs:\n{}", diff),
            Self::UiNoExpectedFile { path } => write!(f, "no expected file: {:?}", path),
            Self::TimeoutExceeded { timeout_secs } => {
                write!(f, "exceeded {}s timeout", timeout_secs)
            }
            Self::CompilationFailed { phase, message } => {
                write!(f, "compilation failed at {}: {}", phase, message)
            }
            Self::RunFailed {
                exit_code,
                expected_exit_code,
            } => {
                write!(
                    f,
                    "run failed: exit code {:?}, expected {:?}",
                    exit_code, expected_exit_code
                )
            }
            Self::StdoutMismatch { expected, actual } => {
                write!(
                    f,
                    "stdout mismatch:\n  expected: {:?}\n  actual:   {:?}",
                    expected, actual
                )
            }
            Self::StderrMismatch { expected, actual } => {
                write!(
                    f,
                    "stderr mismatch:\n  expected: {:?}\n  actual:   {:?}",
                    expected, actual
                )
            }
            Self::RunTimeout { timeout_secs } => {
                write!(f, "run exceeded {}s timeout", timeout_secs)
            }
        }
    }
}

#[derive(Clone, Debug)]
/// TimeoutError.
pub struct TimeoutError {
    /// Struct.
    pub timeout_secs: u64,
}

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "test exceeded {}s timeout", self.timeout_secs)
    }
}
impl std::error::Error for TimeoutError {}

#[derive(Clone, Debug)]
/// AssertionFailure.
pub struct AssertionFailure {
    /// Struct.
    pub expected: String,
    /// Struct.
    pub actual: String,
    /// Struct.
    pub ty_description: String,
}

impl std::error::Error for FailureReason {}
