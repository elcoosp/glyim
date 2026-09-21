use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug)]
/// RunResult.
pub struct RunResult {
    /// Struct.
    pub exit_code: Option<i32>,
    /// Struct.
    pub stdout: String,
    /// Struct.
    pub stderr: String,
    /// Struct.
    pub timed_out: bool,
    /// Struct.
    pub duration: Duration,
}

/// ProgramRunner.
pub struct ProgramRunner {
    program: PathBuf,
    args: Vec<String>,
    stdin_input: Option<String>,
    env: Vec<(String, String)>,
}

impl ProgramRunner {
    /// new.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            stdin_input: None,
            env: Vec::new(),
        }
    }

    /// arg.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// args.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// stdin.
    pub fn stdin(mut self, input: impl Into<String>) -> Self {
        self.stdin_input = Some(input.into());
        self
    }

    /// env.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// run.
    pub fn run(self, timeout: Duration) -> RunResult {
        let start = std::time::Instant::now();

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if self.stdin_input.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return RunResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("failed to spawn process: {}", e),
                    timed_out: false,
                    duration: start.elapsed(),
                };
            }
        };

        if let Some(ref input) = self.stdin_input
            && let Some(mut stdin) = child.stdin.take()
        {
            let _ = stdin.write_all(input.as_bytes());
        }

        let result = run_child_with_timeout(child, timeout);
        let duration = start.elapsed();

        match result {
            ChildResult::Finished(output) => {
                let exit_code = output.status.code();
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                RunResult {
                    exit_code,
                    stdout,
                    stderr,
                    timed_out: false,
                    duration,
                }
            }
            ChildResult::TimedOut => RunResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("process timed out after {}s", timeout.as_secs()),
                timed_out: true,
                duration,
            },
            ChildResult::Error(e) => RunResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("process error: {}", e),
                timed_out: false,
                duration,
            },
        }
    }
}

enum ChildResult {
    Finished(std::process::Output),
    TimedOut,
    Error(String),
}

fn run_child_with_timeout(child: std::process::Child, timeout: Duration) -> ChildResult {
    use std::sync::{Arc, Mutex};
    let (tx, rx) = std::sync::mpsc::channel();
    // Share ownership of the child between the waiter thread and the timeout
    // path (plan §24.2): on timeout we must kill the *actual OS process*
    // rather than only dropping the monitoring thread, otherwise the child
    // (and any descendants) keep running as orphans and become zombies that
    // are never reaped. `Option` lets exactly one side take ownership.
    let child = Arc::new(Mutex::new(Some(child)));
    let child_for_thread = Arc::clone(&child);

    std::thread::spawn(move || {
        let taken = child_for_thread.lock().unwrap().take();
        if let Some(c) = taken {
            let result = c.wait_with_output();
            let _ = tx.send(result);
        }
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => ChildResult::Finished(output),
        Ok(Err(e)) => ChildResult::Error(e.to_string()),
        Err(_) => {
            // Timeout: kill the child so it (and its stdout/stderr pipes) is
            // reaped by the waiter thread's `wait_with_output` after SIGKILL.
            // `take()` ensures we don't race the thread if it already finished.
            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
            ChildResult::TimedOut
        }
    }
}

#[derive(Clone, Debug, Default)]
/// OutputCheck.
pub struct OutputCheck {
    /// Struct.
    pub expected_stdout: Option<String>,
    /// Struct.
    pub expected_stderr: Option<String>,
    /// Struct.
    pub expected_exit_code: Option<i32>,
}

impl OutputCheck {
    /// new.
    pub fn new() -> Self {
        Self::default()
    }

    /// stdout.
    pub fn stdout(mut self, expected: impl Into<String>) -> Self {
        self.expected_stdout = Some(expected.into());
        self
    }

    /// stderr.
    pub fn stderr(mut self, expected: impl Into<String>) -> Self {
        self.expected_stderr = Some(expected.into());
        self
    }

    /// exit_code.
    pub fn exit_code(mut self, code: i32) -> Self {
        self.expected_exit_code = Some(code);
        self
    }

    /// check.
    pub fn check(&self, result: &RunResult) -> Result<(), crate::error::FailureReason> {
        if result.timed_out {
            return Err(crate::error::FailureReason::RunTimeout {
                timeout_secs: result.duration.as_secs(),
            });
        }

        if let Some(expected) = &self.expected_exit_code {
            let actual = result.exit_code.unwrap_or(-1);
            if actual != *expected {
                return Err(crate::error::FailureReason::RunFailed {
                    exit_code: result.exit_code,
                    expected_exit_code: Some(*expected),
                });
            }
        }

        if let Some(expected) = &self.expected_stdout
            && !result.stdout.contains(expected.as_str())
        {
            return Err(crate::error::FailureReason::StdoutMismatch {
                expected: expected.clone(),
                actual: result.stdout.clone(),
            });
        }

        if let Some(expected) = &self.expected_stderr
            && !result.stderr.contains(expected.as_str())
        {
            return Err(crate::error::FailureReason::StderrMismatch {
                expected: expected.clone(),
                actual: result.stderr.clone(),
            });
        }

        Ok(())
    }
}
