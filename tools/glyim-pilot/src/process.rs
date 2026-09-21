use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
/// ProcessError.
pub struct ProcessError {
    /// Struct.
    pub program: String,
    /// Struct.
    pub cwd: std::path::PathBuf,
    /// Struct.
    pub args: Vec<String>,
    /// Struct.
    pub kind: ProcessErrorKind,
}

#[derive(Debug)]
/// ProcessErrorKind.
pub enum ProcessErrorKind {
    #[allow(missing_docs)]
    ExecutionFailed(std::io::Error),
    /// Variant.
    TimedOut {
        /// timeout_secs field.
        timeout_secs: u64,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ProcessErrorKind::ExecutionFailed(e) => write!(
                f,
                "{} failed in {}: {e} (args: {:?})",
                self.program,
                self.cwd.display(),
                self.args
            ),
            ProcessErrorKind::TimedOut { timeout_secs } => write!(
                f,
                "{} timed out after {timeout_secs}s in {} (args: {:?})",
                self.program,
                self.cwd.display(),
                self.args
            ),
        }
    }
}

/// run_timed_command.
pub async fn run_timed_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout_secs: u64,
) -> Result<std::process::Output, ProcessError> {
    let effective_timeout = if timeout_secs == 0 { 300 } else { timeout_secs };
    let timeout = Duration::from_secs(effective_timeout);

    let output_fut = tokio::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output();

    match tokio::time::timeout(timeout, output_fut).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(ProcessError {
            program: program.into(),
            cwd: cwd.to_path_buf(),
            args: args.iter().map(|s| s.to_string()).collect(),
            kind: ProcessErrorKind::ExecutionFailed(e),
        }),
        Err(_) => Err(ProcessError {
            program: program.into(),
            cwd: cwd.to_path_buf(),
            args: args.iter().map(|s| s.to_string()).collect(),
            kind: ProcessErrorKind::TimedOut {
                timeout_secs: effective_timeout,
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_timed_command_success() {
        let result = run_timed_command("echo", &["hello"], Path::new("."), 10).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
    }

    #[tokio::test]
    async fn test_run_timed_command_timeout() {
        let result = run_timed_command("sleep", &["10"], Path::new("."), 1).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err.kind, ProcessErrorKind::TimedOut { .. }));
    }

    #[tokio::test]
    async fn test_run_timed_command_not_found() {
        let result = run_timed_command("nonexistent_command_xyz", &[], Path::new("."), 5).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err.kind, ProcessErrorKind::ExecutionFailed(_)));
    }
}
