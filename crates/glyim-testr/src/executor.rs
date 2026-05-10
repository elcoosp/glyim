use crate::types::{TestOutcome, TestResult};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time;

pub struct Executor {
    bin_path: PathBuf,
    timeout: Duration,
}

impl Executor {
    pub fn new(bin_path: PathBuf, timeout: Duration) -> Self {
        Self { bin_path, timeout }
    }

    /// Spawn a process for a single test, wait with timeout, capture output.
    pub async fn run_test(&self, name: &str) -> Result<TestResult, String> {
        let start = Instant::now();

        let mut child = Command::new(&self.bin_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;

        // Take stdout and stderr handles
        let mut child_stdout = child.stdout.take().unwrap();
        let mut child_stderr = child.stderr.take().unwrap();

        // Spawn tasks to read both streams independently, returning the buffers
        let stdout_reader = tokio::spawn(async move {
            let mut buf = Vec::new();
            child_stdout.read_to_end(&mut buf).await.map(|_| buf)
        });
        let stderr_reader = tokio::spawn(async move {
            let mut buf = Vec::new();
            child_stderr.read_to_end(&mut buf).await.map(|_| buf)
        });

        let wait_fut = child.wait();
        let timeout_fut = time::sleep(self.timeout);

        let exit_status = tokio::select! {
            status = wait_fut => Some(status),
            _ = timeout_fut => {
                child.kill().await.ok();
                None
            }
        };

        // Wait for readers to finish
        let _stdout_buf = stdout_reader.await.unwrap().unwrap();
        let stderr_buf = stderr_reader.await.unwrap().unwrap();

        match exit_status {
            Some(Ok(status)) => {
                let stderr_str = String::from_utf8_lossy(&stderr_buf).to_string();
                if status.success() {
                    Ok(TestResult {
                        name: name.into(),
                        outcome: TestOutcome::Passed,
                        duration: start.elapsed(),
                    })
                } else {
                    Ok(TestResult {
                        name: name.into(),
                        outcome: TestOutcome::Failed {
                            exit_code: status.code().unwrap_or(1),
                            stderr: stderr_str,
                        },
                        duration: start.elapsed(),
                    })
                }
            }
            Some(Err(e)) => Err(format!("child wait error: {e}")),
            None => Ok(TestResult {
                name: name.into(),
                outcome: TestOutcome::TimedOut,
                duration: start.elapsed(),
            }),
        }
    }
}
