use crate::types::{TestOutcome, TestResult};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
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
            .env("GLYIM_TEST", name)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let (stdout_tx, mut stdout_rx) = mpsc::unbounded_channel::<String>();
        let stdout_tx_clone = stdout_tx.clone();

        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if stdout_tx_clone.send(line).is_err() {
                    break;
                }
            }
        });

        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = tokio::io::BufReader::new(stderr).read_to_end(&mut buf).await;
            buf
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

        drop(stdout_tx);
        let _ = stdout_task.await;
        let mut stdout_lines = Vec::new();
        while let Ok(line) = stdout_rx.try_recv() {
            stdout_lines.push(line);
        }

        let stderr_buf = stderr_task.await.map_err(|e| format!("stderr task: {e}"))?;

        match exit_status {
            Some(Ok(status)) => {
                let stderr_str = String::from_utf8_lossy(&stderr_buf).to_string();
                if status.success() {
                    let passed = stdout_lines.iter().any(|l| l.trim() == format!("PASS {}", name));
                    if passed {
                        Ok(TestResult {
                            name: name.into(),
                            outcome: TestOutcome::Passed,
                            duration: start.elapsed(),
                        })
                    } else {
                        Ok(TestResult {
                            name: name.into(),
                            outcome: TestOutcome::InternalError(
                                "harness did not print PASS".into()
                            ),
                            duration: start.elapsed(),
                        })
                    }
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
