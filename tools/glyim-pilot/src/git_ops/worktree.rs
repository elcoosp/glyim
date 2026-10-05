use crate::error::PilotError;
use crate::process::run_timed_command;
use std::path::{Path, PathBuf};

/// create_worktree.
pub async fn create_worktree(
    repo_root: &Path,
    worktree_base: &Path,
    stream_id: &str,
    default_branch: &str,
    branch_version: &str,
    timeout_secs: u64,
) -> Result<PathBuf, PilotError> {
    // T016-PATCHED-WORKTREE [PILOT-1]: defense-in-depth validation. Even
    // though the WebSocket layer now rejects malformed ids, the worktree
    // layer must never trust the caller — a `stream_id` like `../../tmp/evil`
    // would escape `worktree_base` and a `branch_version` with `/`, spaces,
    // or `..` would break `git checkout -b`.
    crate::error::validate_id(stream_id)?;
    crate::error::validate_id(branch_version)?;
    let worktree_dir = worktree_base.join(format!("stream-{stream_id}"));
    let branch_name = format!("stream-{stream_id}/{branch_version}");
    let args = &[
        "worktree",
        "add",
        "--detach",
        &worktree_dir.to_string_lossy(),
        default_branch,
    ];
    let output = run_timed_command("git", args, repo_root, timeout_secs)
        .await
        .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let checkout_args = &["checkout", "-b", &branch_name];
    let output = run_timed_command("git", checkout_args, &worktree_dir, timeout_secs)
        .await
        .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git checkout -b failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(worktree_dir)
}

/// commit_all.
pub async fn commit_all(
    worktree_dir: &Path,
    stream_id: &str,
    message: &str,
    timeout_secs: u64,
) -> Result<(), PilotError> {
    let commit_msg = format!("stream-{stream_id}: {message}");
    let output = run_timed_command("git", &["add", "-A"], worktree_dir, timeout_secs)
        .await
        .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let output = run_timed_command(
        "git",
        &["commit", "-m", &commit_msg],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("nothing to commit") {
            return Ok(());
        }
        return Err(PilotError::Git(format!("git commit failed: {stderr}")));
    }
    Ok(())
}

/// emergency_wip_commit.
pub async fn emergency_wip_commit(
    worktree_dir: &Path,
    stream_id: &str,
    timeout_secs: u64,
) -> Result<(), PilotError> {
    commit_all(
        worktree_dir,
        stream_id,
        "WIP: emergency commit — fix rounds exceeded",
        timeout_secs,
    )
    .await
}

/// push_branch.
pub async fn push_branch(
    worktree_dir: &Path,
    stream_id: &str,
    branch_version: &str,
    timeout_secs: u64,
) -> Result<(), PilotError> {
    let branch_name = format!("stream-{stream_id}/{branch_version}");
    let output = run_timed_command(
        "git",
        &["push", "-u", "origin", &branch_name],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git push failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// create_pr.
pub async fn create_pr(
    worktree_dir: &Path,
    stream_id: &str,
    default_branch: &str,
    branch_version: &str,
    title: &str,
    body: &str,
    timeout_secs: u64,
) -> Result<String, PilotError> {
    let branch_name = format!("stream-{stream_id}/{branch_version}");
    let output = run_timed_command(
        "gh",
        &[
            "pr",
            "create",
            "--base",
            default_branch,
            "--head",
            &branch_name,
            "--title",
            title,
            "--body",
            body,
        ],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "gh pr create failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// status_porcelain.
pub async fn status_porcelain(
    worktree_dir: &Path,
    timeout_secs: u64,
) -> Result<String, PilotError> {
    let output = run_timed_command(
        "git",
        &["status", "--porcelain"],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// diff_main.
pub async fn diff_main(
    worktree_dir: &Path,
    default_branch: &str,
    timeout_secs: u64,
) -> Result<String, PilotError> {
    let output = run_timed_command(
        "git",
        &["diff", &format!("{default_branch}..HEAD")],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// log_oneline.
pub async fn log_oneline(
    worktree_dir: &Path,
    default_branch: &str,
    timeout_secs: u64,
) -> Result<String, PilotError> {
    let output = run_timed_command(
        "git",
        &["log", &format!("{default_branch}..HEAD"), "--oneline"],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// diff_name_only.
pub async fn diff_name_only(
    worktree_dir: &Path,
    default_branch: &str,
    timeout_secs: u64,
) -> Result<String, PilotError> {
    let output = run_timed_command(
        "git",
        &["diff", "--name-only", &format!("{default_branch}..HEAD")],
        worktree_dir,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git diff --name-only failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// remove_worktree.
pub async fn remove_worktree(
    repo_root: &Path,
    worktree_dir: &Path,
    timeout_secs: u64,
) -> Result<(), PilotError> {
    let output = run_timed_command(
        "git",
        &[
            "worktree",
            "remove",
            &worktree_dir.to_string_lossy(),
            "--force",
        ],
        repo_root,
        timeout_secs,
    )
    .await
    .map_err(|e| PilotError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(PilotError::Git(format!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// detect_default_branch.
pub async fn detect_default_branch(repo_root: &Path, fallback: &str, timeout_secs: u64) -> String {
    match run_timed_command(
        "git",
        &["symbolic-ref", "refs/remotes/origin/HEAD"],
        repo_root,
        timeout_secs,
    )
    .await
    {
        Ok(output) if output.status.success() => {
            let ref_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ref_path
                .strip_prefix("refs/remotes/origin/")
                .map(|s| s.to_string())
                .unwrap_or_else(|| fallback.to_string())
        }
        _ => fallback.to_string(),
    }
}
