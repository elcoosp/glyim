use crate::error::PilotError;
use crate::process::run_timed_command;
use std::path::Path;

/// `run_gate_command`.
pub async fn run_gate_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout_secs: u64,
    gate_name: &str,
) -> Result<std::process::Output, PilotError> {
    run_timed_command(program, args, cwd, timeout_secs)
        .await
        .map_err(|e| PilotError::Gate {
            gate: gate_name.into(),
            message: e.to_string(),
        })
}

/// `strip_ansi`.
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    strip_ansi_escapes::strip_str(s)
}

/// `trim_errors_and_warnings`.
#[must_use]
pub fn trim_errors_and_warnings(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 50 {
        return output.to_string();
    }
    let mut relevant = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("error") || trimmed.starts_with("warning") {
            let start = i.saturating_sub(2);
            let end = (i + 5).min(lines.len());
            for &line in lines.iter().take(end).skip(start) {
                relevant.push(line);
            }
            relevant.push("...");
        }
    }
    if relevant.is_empty() {
        lines[lines.len() - 50..].join("\n")
    } else {
        relevant.join("\n")
    }
}

/// `is_command_not_found`.
#[must_use]
pub fn is_command_not_found(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}").to_lowercase();
    combined.contains("command not found")
        || combined.contains("no such command")
        || combined.contains("not found")
}

/// `trim_test_failures`.
#[must_use]
pub fn trim_test_failures(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 80 {
        return output.to_string();
    }
    let mut relevant = Vec::new();
    let mut in_failures = false;
    for line in &lines {
        if line.trim().starts_with("failures:") {
            in_failures = true;
        }
        if in_failures {
            relevant.push(*line);
            if relevant.len() > 60 {
                break;
            }
        }
    }
    for line in lines.iter().rev() {
        if line.contains("test result:") {
            relevant.push(*line);
            break;
        }
    }
    if relevant.is_empty() {
        lines[lines.len() - 60..].join("\n")
    } else {
        relevant.join("\n")
    }
}
